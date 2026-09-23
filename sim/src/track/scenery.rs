//! Scenery: the props that stand beside a circuit.
//!
//! Props are decoration and nothing else. The simulation never sees them: they stand outside the
//! walls, where no car can reach, so adding one can never change how a track drives. They are
//! written in the track file next to the segments that draw the road, and reach clients with the
//! rest of the description (see `docs/tracks.md`).
//!
//! A prop is placed the way a marshal would describe a spot on a circuit: how far along the lap it
//! is, which side of the road it stands on, and how far back from the wall. Nothing is written in
//! world coordinates, so a prop keeps its place when the road around it is redrawn, and the editor
//! to come can move one by dragging it along the road.

use bitcode::{Decode, Encode};
use serde::Deserialize;

/// Tallest, longest or widest a prop may be, in meters. Past this a file is more likely wrong than
/// ambitious.
const MAX_SIZE: f32 = 250.0;
/// Farthest a prop may stand from the road, in meters.
const MAX_OFFSET: f32 = 500.0;
/// A grandstand of more rows, or a marker board of more chevrons, is a typo.
const MAX_ROWS: u8 = 24;
const MAX_CHEVRONS: u8 = 8;
/// A gantry beam must clear the walls by at least this much, in meters.
const MIN_CLEARANCE: f32 = 4.0;

/// Which side of the road a prop stands on, looking along the driving direction.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Encode, Decode)]
pub enum Side {
    Left,
    Right,
}

impl Side {
    /// `1` on the left and `-1` on the right, the sign of a lateral offset from the centerline (see
    /// [`TrackProjection::lateral`](super::TrackProjection::lateral)).
    pub fn sign(self) -> f32 {
        match self {
            Self::Left => 1.0,
            Self::Right => -1.0,
        }
    }
}

/// One piece of scenery, with everything the renderer needs to build it.
///
/// Every prop but the gantry, which spans the road, is placed by three numbers: `at`, meters along
/// the lap from the start line; `side`; and `offset`, meters from the outer face of the wall to the
/// face of the prop that looks at the road. An `offset` of zero therefore leaves the prop leaning
/// against the wall, and no prop ever overhangs the road.
///
/// The sizes have defaults, so a file writes only what it wants different.
#[derive(Debug, Clone, Copy, PartialEq, Deserialize, Encode, Decode)]
#[serde(deny_unknown_fields)]
pub enum Prop {
    /// An arch over the road, its legs standing just outside the walls. The landmark of a circuit:
    /// it is seen from far down a straight, and the road running under it marks where a lap starts,
    /// or where a turn is coming.
    Gantry {
        at: f32,
        /// How far the beam clears the higher of the two walls, in meters. Measured from the wall,
        /// so a gantry over a banked turn rises with it.
        #[serde(default = "gantry_clearance")]
        clearance: f32,
        /// How thick the legs and the beam are along the road, in meters.
        #[serde(default = "gantry_depth")]
        depth: f32,
    },
    /// A tapered mast ringed with neon. The cheapest way to make a turn entry visible from far down
    /// the road, and, going by at speed, to show how fast the car is travelling.
    Pylon {
        at: f32,
        side: Side,
        #[serde(default = "pylon_offset")]
        offset: f32,
        #[serde(default = "pylon_height")]
        height: f32,
    },
    /// A stepped tribune following the curve of the road, a neon line along the front of every row.
    /// Around a long turn it draws the curve the driver is about to take.
    Grandstand {
        at: f32,
        side: Side,
        #[serde(default = "grandstand_offset")]
        offset: f32,
        /// How far it runs along the road, in meters.
        #[serde(default = "grandstand_length")]
        length: f32,
        #[serde(default = "grandstand_rows")]
        rows: u8,
    },
    /// A panel on two posts, its neon frame facing the road. Beside a straight it gives the eye
    /// something to measure speed against.
    Billboard {
        at: f32,
        side: Side,
        #[serde(default = "billboard_offset")]
        offset: f32,
        #[serde(default = "billboard_width")]
        width: f32,
        #[serde(default = "billboard_height")]
        height: f32,
    },
    /// A marker board of stacked neon arrows, pointing the way the road bends. The renderer reads
    /// the bend from the centerline, so a board placed in a turn always points into it.
    Chevrons {
        at: f32,
        side: Side,
        #[serde(default = "chevrons_offset")]
        offset: f32,
        #[serde(default = "chevrons_count")]
        count: u8,
    },
    /// A tall slab standing far out in the dark, a neon seam up the face that looks at the road.
    /// Nothing to drive by: it gives the black around the circuit a depth, and the turns something
    /// to turn against.
    Monolith {
        at: f32,
        side: Side,
        #[serde(default = "monolith_offset")]
        offset: f32,
        #[serde(default = "monolith_height")]
        height: f32,
        #[serde(default = "monolith_width")]
        width: f32,
    },
}

const fn gantry_clearance() -> f32 {
    7.0
}

const fn gantry_depth() -> f32 {
    2.0
}

const fn pylon_offset() -> f32 {
    3.0
}

const fn pylon_height() -> f32 {
    16.0
}

const fn grandstand_offset() -> f32 {
    5.0
}

const fn grandstand_length() -> f32 {
    40.0
}

const fn grandstand_rows() -> u8 {
    6
}

const fn billboard_offset() -> f32 {
    4.0
}

const fn billboard_width() -> f32 {
    10.0
}

const fn billboard_height() -> f32 {
    5.0
}

const fn chevrons_offset() -> f32 {
    1.5
}

const fn chevrons_count() -> u8 {
    3
}

const fn monolith_offset() -> f32 {
    70.0
}

const fn monolith_height() -> f32 {
    32.0
}

const fn monolith_width() -> f32 {
    12.0
}

impl Prop {
    /// Distance along the lap where the prop stands, in meters from the start line.
    pub fn at(&self) -> f32 {
        match *self {
            Self::Gantry { at, .. }
            | Self::Pylon { at, .. }
            | Self::Grandstand { at, .. }
            | Self::Billboard { at, .. }
            | Self::Chevrons { at, .. }
            | Self::Monolith { at, .. } => at,
        }
    }

    /// The side of the road the prop stands on, or `None` for a prop that spans it.
    pub fn side(&self) -> Option<Side> {
        match *self {
            Self::Gantry { .. } => None,
            Self::Pylon { side, .. }
            | Self::Grandstand { side, .. }
            | Self::Billboard { side, .. }
            | Self::Chevrons { side, .. }
            | Self::Monolith { side, .. } => Some(side),
        }
    }

    /// Meters between the outer face of the wall and the prop, or `None` for a prop that spans the
    /// road.
    pub fn offset(&self) -> Option<f32> {
        match *self {
            Self::Gantry { .. } => None,
            Self::Pylon { offset, .. }
            | Self::Grandstand { offset, .. }
            | Self::Billboard { offset, .. }
            | Self::Chevrons { offset, .. }
            | Self::Monolith { offset, .. } => Some(offset),
        }
    }

    /// Checks a prop against a track `length` meters round: it must stand somewhere on the lap,
    /// beside the road rather than on it, and have sizes a renderer can build.
    pub fn validate(&self, length: f32) -> Result<(), &'static str> {
        if !(self.at().is_finite() && (0.0..=length).contains(&self.at())) {
            return Err("the distance along the track must be between zero and its length");
        }
        if let Some(offset) = self.offset()
            && !(offset.is_finite() && (0.0..=MAX_OFFSET).contains(&offset))
        {
            return Err("the offset must be between zero, against the wall, and 500 meters");
        }
        match *self {
            Self::Gantry {
                clearance, depth, ..
            } => {
                if !(clearance.is_finite() && (MIN_CLEARANCE..=MAX_SIZE).contains(&clearance)) {
                    return Err("the clearance must leave at least 4 meters over the walls");
                }
                size(depth)?;
            }
            Self::Pylon { height, .. } => size(height)?,
            Self::Grandstand { length, rows, .. } => {
                size(length)?;
                count(rows, MAX_ROWS)?;
            }
            Self::Billboard { width, height, .. } => {
                size(width)?;
                size(height)?;
            }
            Self::Chevrons {
                count: chevrons, ..
            } => count(chevrons, MAX_CHEVRONS)?,
            Self::Monolith { height, width, .. } => {
                size(height)?;
                size(width)?;
            }
        }
        Ok(())
    }
}

fn size(value: f32) -> Result<(), &'static str> {
    if value.is_finite() && value > 0.0 && value <= MAX_SIZE {
        Ok(())
    } else {
        Err("every size must be a positive number of meters, at most 250")
    }
}

fn count(value: u8, max: u8) -> Result<(), &'static str> {
    if (1..=max).contains(&value) {
        Ok(())
    } else {
        Err("a prop needs at least one row or chevron, and at most a couple of dozen")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_prop_writes_only_what_it_wants_different() {
        let prop: Prop = ron::from_str("Pylon(at: 40.0, side: Left)").unwrap();
        assert_eq!(
            prop,
            Prop::Pylon {
                at: 40.0,
                side: Side::Left,
                offset: pylon_offset(),
                height: pylon_height(),
            }
        );
    }

    #[test]
    fn a_typo_in_a_prop_is_an_error() {
        let error = ron::from_str::<Prop>("Pylon(at: 40.0, side: Left, heigth: 12.0)").unwrap_err();
        assert!(error.to_string().contains("heigth"), "{error}");
    }

    #[test]
    fn a_prop_must_stand_on_the_lap_and_off_the_road() {
        let pylon = |at, offset| Prop::Pylon {
            at,
            side: Side::Right,
            offset,
            height: 12.0,
        };
        assert!(pylon(100.0, 0.0).validate(500.0).is_ok());
        assert!(pylon(600.0, 0.0).validate(500.0).is_err());
        assert!(pylon(-1.0, 0.0).validate(500.0).is_err());
        // A negative offset would stand the prop over the road.
        assert!(pylon(100.0, -2.0).validate(500.0).is_err());
    }

    #[test]
    fn a_prop_must_have_sizes_a_renderer_can_build() {
        let grandstand = |length, rows| Prop::Grandstand {
            at: 10.0,
            side: Side::Left,
            offset: 4.0,
            length,
            rows,
        };
        assert!(grandstand(30.0, 5).validate(500.0).is_ok());
        assert!(grandstand(0.0, 5).validate(500.0).is_err());
        assert!(grandstand(f32::NAN, 5).validate(500.0).is_err());
        assert!(grandstand(30.0, 0).validate(500.0).is_err());
        assert!(grandstand(30.0, MAX_ROWS + 1).validate(500.0).is_err());
    }

    #[test]
    fn a_gantry_beam_stays_clear_of_the_walls() {
        let gantry = |clearance| Prop::Gantry {
            at: 0.0,
            clearance,
            depth: 2.0,
        };
        assert!(gantry(MIN_CLEARANCE).validate(500.0).is_ok());
        assert!(gantry(1.0).validate(500.0).is_err());
    }

    #[test]
    fn sides_carry_the_sign_of_a_lateral_offset() {
        assert_eq!(Side::Left.sign(), 1.0);
        assert_eq!(Side::Right.sign(), -1.0);
    }
}
