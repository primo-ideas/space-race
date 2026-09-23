//! The driving simulation. The server runs it with full authority; it is a crate of its own so
//! the client can run the exact same code later, for prediction.
//!
//! It knows nothing of the wire: `space-race-protocol` depends on it, never the reverse. What
//! crosses the wire derives its encoding here all the same, so a snapshot carries the
//! simulation's own `Car` rather than a copy that could drift out of step.
//!
//! Everything happens on a flat plane: `x` and `y` in meters, angles in radians counter-clockwise
//! from `+x`, so a positive angle turns left.

pub mod autopilot;
pub mod car;
pub mod race;
pub mod track;

#[cfg(test)]
mod test_support;

use std::time::Duration;

/// Simulation steps per second.
pub const TICK_RATE: u32 = 60;

/// Duration of one simulation step, in seconds.
pub const TICK_SECONDS: f32 = 1.0 / TICK_RATE as f32;

pub const TICK_DURATION: Duration = Duration::from_nanos(1_000_000_000 / TICK_RATE as u64);
