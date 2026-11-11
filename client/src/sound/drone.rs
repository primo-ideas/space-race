//! The sound that never stops while a car is on screen: its thrusters, the air it pushes through
//! and the hiss of a drift. One stream, played from the start of the game to its end, whose voices
//! follow a handful of numbers the game sets every frame; with nothing to hear it plays silence.
//!
//! It is not the engine of v0.1.6, which fired like a piston engine and was taken out. The car is
//! a hovercraft on thrusters now, and this is a turbine: a low hum with two harmonics and a thin
//! whine far above it, all rising with the speed, under a wind that opens up as the car goes
//! faster. Nothing in it knocks.

use std::f32::consts::TAU;
use std::num::NonZero;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;

use bevy::audio::{ChannelCount, Decodable, Sample, SampleRate, Source};
use bevy::prelude::*;

use super::tone::{Filter, Hiss, SAMPLE_RATE};

/// How loud the whole drone is at its fullest: under the one-shot sounds, which must stand out.
const AMPLITUDE: f32 = 0.2;
/// Seconds the voices take to follow what the game asks: frames come 60 times a second and
/// samples 44,100, and a voice that jumped from one frame's value to the next would crackle.
const GLIDE: f32 = 0.06;
/// The game's numbers are read again every this many samples.
const READ_EVERY: u32 = 64;

/// What the drone is asked to sound like, set by the game every frame.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Drive {
    /// 1 while there is a car to hear, 0 otherwise.
    pub on: f32,
    /// The car's speed as a share of its top speed: beyond 1 on a boost or down a dive.
    pub speed: f32,
    /// How hard a boost pushes: 0 none, 0.6 a drift's or a start's, 1 the turbo.
    pub boost: f32,
    /// 1 while the car drifts.
    pub drift: f32,
    /// How far the drift has charged, from 0 to 1: the hiss rises with it.
    pub charge: f32,
}

/// [`Drive`] as the audio thread reads it while the game writes it: one number at a time, each
/// whole, which is all a sound gliding toward them needs.
#[derive(Default)]
pub struct Controls([AtomicU32; 5]);

impl Controls {
    pub fn set(&self, drive: Drive) {
        let values = [
            drive.on,
            drive.speed,
            drive.boost,
            drive.drift,
            drive.charge,
        ];
        for (slot, value) in self.0.iter().zip(values) {
            slot.store(value.to_bits(), Ordering::Relaxed);
        }
    }

    fn get(&self) -> Drive {
        let [on, speed, boost, drift, charge] =
            [0, 1, 2, 3, 4].map(|slot| f32::from_bits(self.0[slot].load(Ordering::Relaxed)));
        Drive {
            on,
            speed,
            boost,
            drift,
            charge,
        }
    }
}

#[derive(Asset, TypePath, Clone)]
pub struct Drone(pub Arc<Controls>);

impl Decodable for Drone {
    type Decoder = DroneSamples;

    fn decoder(&self) -> Self::Decoder {
        DroneSamples {
            controls: self.0.clone(),
            asked: Drive::default(),
            now: Drive::default(),
            sample: 0,
            hum: 0.0,
            whine: 0.0,
            scrub: 0.0,
            air: Hiss::new(0xa1e),
            grit: Hiss::new(0x9217),
            wind: Filter::default(),
            skid: Filter::default(),
        }
    }
}

/// The drone, played out sample by sample, for as long as anyone listens.
pub struct DroneSamples {
    controls: Arc<Controls>,
    /// What the game last asked for, and what is sounding now, gliding toward it.
    asked: Drive,
    now: Drive,
    sample: u32,
    /// Where the hum, the whine and the drift's scrubbing are in their cycles, from 0 to 1.
    hum: f32,
    whine: f32,
    scrub: f32,
    air: Hiss,
    grit: Hiss,
    wind: Filter,
    skid: Filter,
}

impl Iterator for DroneSamples {
    type Item = Sample;

    fn next(&mut self) -> Option<Sample> {
        if self.sample.is_multiple_of(READ_EVERY) {
            self.asked = self.controls.get();
        }
        self.sample = self.sample.wrapping_add(1);
        let glide = 1.0 / (GLIDE * SAMPLE_RATE as f32);
        let follow = |now: &mut f32, asked: f32| *now += (asked - *now) * glide;
        follow(&mut self.now.on, self.asked.on);
        follow(&mut self.now.speed, self.asked.speed.clamp(0.0, 2.0));
        follow(&mut self.now.boost, self.asked.boost);
        follow(&mut self.now.drift, self.asked.drift);
        follow(&mut self.now.charge, self.asked.charge);
        let Drive {
            on,
            speed,
            boost,
            drift,
            charge,
        } = self.now;
        if on < 1e-4 {
            return Some(0.0);
        }
        let step = 1.0 / SAMPLE_RATE as f32;

        // The thrusters: a hum from 55 Hz standing still to 130 at top speed, a fifth higher under
        // the turbo, with the two harmonics that let a small speaker say it at all, and a thin
        // whine seven times higher that comes in with the speed.
        let pitch = (55.0 + 75.0 * speed) * (1.0 + 0.18 * boost);
        self.hum = (self.hum + pitch * step).fract();
        self.whine = (self.whine + pitch * 7.03 * step).fract();
        let hum = (TAU * self.hum).sin()
            + 0.45 * (TAU * 2.0 * self.hum + 0.3).sin()
            + 0.22 * (TAU * 3.0 * self.hum).sin();
        let whine = (TAU * self.whine).sin() * (0.03 + 0.07 * speed.min(1.5));
        let thrusters = hum * (0.16 + 0.22 * speed.min(1.3) + 0.1 * boost) + whine;

        // The air: nothing standing still, a low rush at speed, opening up and roaring on a boost.
        let (rush, _) = self.wind.run(
            self.air.next(),
            250.0 + 1800.0 * speed * speed + 1500.0 * boost,
            1.2,
        );
        let wind = rush * (0.3 * speed * speed + 0.5 * boost);

        // The drift: a narrow hiss that scrubs, and rises as the drift charges.
        self.scrub = (self.scrub + 23.0 * step).fract();
        let (_, hiss) = self
            .skid
            .run(self.grit.next(), 1500.0 + 1300.0 * charge, 0.25);
        let skid = hiss * 0.2 * drift * (0.8 + 0.2 * (TAU * self.scrub).sin());

        Some((AMPLITUDE * on * (thrusters + wind + skid)).clamp(-1.0, 1.0))
    }
}

impl Source for DroneSamples {
    fn current_span_len(&self) -> Option<usize> {
        None
    }

    fn channels(&self) -> ChannelCount {
        NonZero::new(1).expect("one channel")
    }

    fn sample_rate(&self) -> SampleRate {
        NonZero::new(SAMPLE_RATE).expect("a sample rate above zero")
    }

    /// It never ends.
    fn total_duration(&self) -> Option<Duration> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A second of the drone, a second after the game asked for `drive`, and how loud it is.
    fn loudness(drive: Drive) -> (f32, f32) {
        let controls = Arc::new(Controls::default());
        controls.set(drive);
        let samples: Vec<f32> = Drone(controls)
            .decoder()
            .skip(SAMPLE_RATE as usize)
            .take(SAMPLE_RATE as usize)
            .collect();
        let peak = samples
            .iter()
            .fold(0.0f32, |peak, sample| peak.max(sample.abs()));
        let mean = (samples.iter().map(|sample| sample * sample).sum::<f32>()
            / samples.len() as f32)
            .sqrt();
        (peak, mean)
    }

    /// Silent with no car to hear; quiet standing on the grid, louder at speed, louder still
    /// drifting on the turbo, and never loud enough to clip or to drown the other sounds.
    #[test]
    fn the_drone_is_silent_off_and_grows_with_the_speed_without_clipping() {
        let car = |speed: f32| Drive {
            on: 1.0,
            speed,
            ..default()
        };
        assert_eq!(loudness(Drive::default()), (0.0, 0.0));
        let (_, standing) = loudness(car(0.0));
        let (_, racing) = loudness(car(1.0));
        let (peak, flat_out) = loudness(Drive {
            boost: 1.0,
            drift: 1.0,
            charge: 1.0,
            ..car(1.55)
        });
        println!("standing {standing}, racing {racing}, flat out {flat_out}, peak {peak}");
        assert!(standing > 0.01 && standing < racing && racing < flat_out);
        assert!(peak < 0.6, "{peak}");
    }

    /// What the game asks for is glided to, not jumped to: no click when a car appears.
    #[test]
    fn a_car_appearing_fades_in() {
        let controls = Arc::new(Controls::default());
        let mut samples = Drone(controls.clone()).decoder();
        assert_eq!(samples.next(), Some(0.0));
        controls.set(Drive {
            on: 1.0,
            speed: 1.0,
            ..default()
        });
        let first: Vec<f32> = samples.by_ref().take(400).collect();
        let steepest = first
            .windows(2)
            .fold(0.0f32, |most, pair| most.max((pair[1] - pair[0]).abs()));
        assert!(steepest < 0.01, "{steepest}");
    }
}
