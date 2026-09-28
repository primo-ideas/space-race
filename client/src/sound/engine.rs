//! The engine, synthesized sample by sample. It is kept apart from Bevy, like the prediction, so it
//! can be tested, and listened to, on its own.
//!
//! An engine is a train of explosions, and so is this one: a knock on every firing, rung through
//! three resonances that give it its voice, the body, the exhaust pipe and, as the engine works
//! harder, the rasp of the intake. The firings come faster as the engine turns faster, which is
//! the pitch the player hears climb with the speed, while the resonances stay where they are, which
//! is what makes it one engine at every speed rather than one note played higher or lower. No two
//! firings are quite alike: each is a little stronger or weaker, a little early or late, and every
//! other one weaker still, the uneven beat that makes an engine burble rather than buzz.

use std::f32::consts::TAU;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

use super::SAMPLE_RATE;

/// Firings a second at idle and at the car's top speed: a four-cylinder turning at 900 and at
/// 4,050 rpm. A boost takes it past the second, since the revs go on following the speed.
const IDLE_FIRINGS: f32 = 30.0;
const TOP_SPEED_FIRINGS: f32 = 135.0;
/// How much stronger or weaker a firing can be than the steady one, and how much earlier or later
/// it can come, as shares.
const STRENGTH_JITTER: f32 = 0.2;
const TIMING_JITTER: f32 = 0.02;
/// How much weaker every other firing is: the engine's lope.
const UNEVEN: f32 = 0.3;
/// The hiss of the gas behind each knock under load, as a share of the knock, and how quickly it
/// dies away, per second.
const HISS: f32 = 0.05;
const HISS_DECAY: f32 = 250.0;

/// How the engine rings.
struct Resonance {
    /// In Hz.
    frequency: f32,
    /// How narrow the band is: the higher, the longer it rings after a knock.
    sharpness: f32,
    /// How loudly it rings when the engine coasts, and with the accelerator pressed.
    coasting: f32,
    loaded: f32,
}

const RESONANCES: [Resonance; 3] = [
    // The body: deep, broad and always there.
    Resonance {
        frequency: 110.0,
        sharpness: 2.0,
        coasting: 1.0,
        loaded: 1.0,
    },
    // The exhaust pipe, which rings longer, louder under load.
    Resonance {
        frequency: 340.0,
        sharpness: 5.0,
        coasting: 0.3,
        loaded: 0.6,
    },
    // The intake, a rasp that comes with the load.
    Resonance {
        frequency: 1300.0,
        sharpness: 2.0,
        coasting: 0.02,
        loaded: 0.12,
    },
];

/// How hard the resonances drive the output, which rounds off their peaks (see `sample`).
const DRIVE: f32 = 0.25;
/// How loud the engine is at its loudest, leaving room for the other sounds.
const AMPLITUDE: f32 = 0.35;
/// How loud it is coasting, as a share of the same revs under load.
const COASTING_LOUDNESS: f32 = 0.55;

/// Per second: how quickly the engine follows what the game asks of it. The revs have the
/// engine's own inertia; the load follows the accelerator almost at once.
const REVS_RESPONSE: f32 = 10.0;
const LOAD_RESPONSE: f32 = 25.0;
const LEVEL_RESPONSE: f32 = 10.0;
/// Below this level, a silenced engine stops sounding altogether.
const SILENCE: f32 = 1e-4;

/// How the engine is running, as the game sets it.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Running {
    /// How fast it turns: 0 at idle, 1 at the car's top speed, more on a boost.
    pub revs: f32,
    /// How hard it works: 1 with the accelerator pressed, 0 coasting.
    pub load: f32,
    /// How loud it is, from 0, silent, to 1.
    pub level: f32,
}

/// [`Running`], shared between the game, which sets it once a frame, and the sound, which reads it
/// at every sample, on a thread of its own natively.
#[derive(Default)]
pub struct Controls([AtomicU32; 3]);

impl Controls {
    pub fn set(&self, running: Running) {
        let [revs, load, level] = &self.0;
        revs.store(running.revs.to_bits(), Ordering::Relaxed);
        load.store(running.load.to_bits(), Ordering::Relaxed);
        level.store(running.level.to_bits(), Ordering::Relaxed);
    }

    fn get(&self) -> Running {
        let [revs, load, level] = &self.0;
        Running {
            revs: f32::from_bits(revs.load(Ordering::Relaxed)),
            load: f32::from_bits(load.load(Ordering::Relaxed)),
            level: f32::from_bits(level.load(Ordering::Relaxed)),
        }
    }
}

/// The engine as it sounds, one sample after another, following its [`Controls`].
pub struct Engine {
    controls: Arc<Controls>,
    /// What it does now, following the controls smoothly.
    running: Running,
    /// How far it is from one firing to the next, from 0 to 1.
    phase: f32,
    /// How much longer this cycle is than the steady one: firings come a little early or late.
    stretch: f32,
    /// Whether the next firing is one of the weaker ones.
    weak: bool,
    /// The hiss behind the last firing, dying away.
    hiss: f32,
    /// The state of a small random generator (xorshift), for the jitter and the hiss.
    random: u32,
    resonators: [Resonator; 3],
}

impl Engine {
    /// A silent engine, until its controls say otherwise.
    pub fn new(controls: Arc<Controls>) -> Self {
        Self {
            controls,
            running: Running::default(),
            phase: 0.0,
            stretch: 1.0,
            weak: false,
            hiss: 0.0,
            random: 0x9e37_79b9,
            resonators: RESONANCES
                .map(|resonance| Resonator::new(resonance.frequency, resonance.sharpness)),
        }
    }

    /// The next sample, between `-AMPLITUDE` and `AMPLITUDE`.
    pub fn sample(&mut self) -> f32 {
        let target = self.controls.get();
        let follow = |now: f32, target: f32, response: f32| {
            now + (target - now) * response / SAMPLE_RATE as f32
        };
        let running = Running {
            revs: follow(self.running.revs, target.revs, REVS_RESPONSE),
            load: follow(self.running.load, target.load, LOAD_RESPONSE),
            level: follow(self.running.level, target.level, LEVEL_RESPONSE),
        };
        self.running = running;
        if running.level < SILENCE && target.level <= 0.0 {
            return 0.0;
        }

        let firings = IDLE_FIRINGS + (TOP_SPEED_FIRINGS - IDLE_FIRINGS) * running.revs.max(0.0);
        self.phase += firings / (SAMPLE_RATE as f32 * self.stretch);
        let mut knock = 0.0;
        if self.phase >= 1.0 {
            self.phase -= 1.0;
            self.stretch = 1.0 + TIMING_JITTER * self.random();
            knock = 1.0 + STRENGTH_JITTER * self.random();
            if self.weak {
                knock *= 1.0 - UNEVEN;
            }
            self.weak = !self.weak;
            self.hiss = knock;
        }
        self.hiss *= 1.0 - HISS_DECAY / SAMPLE_RATE as f32;
        let excitation = knock + HISS * running.load * self.hiss * self.random();

        let rung: f32 = self
            .resonators
            .iter_mut()
            .zip(&RESONANCES)
            .map(|(resonator, resonance)| {
                let share =
                    resonance.coasting + (resonance.loaded - resonance.coasting) * running.load;
                share * resonator.ring(excitation)
            })
            .sum();
        let loudness =
            running.level * (COASTING_LOUDNESS + (1.0 - COASTING_LOUDNESS) * running.load);
        // The peaks are rounded off rather than cut, which also keeps the sound within its range
        // whatever the resonances do.
        AMPLITUDE * loudness * (DRIVE * rung).tanh()
    }

    /// A random number from -1 to 1.
    fn random(&mut self) -> f32 {
        self.random ^= self.random << 13;
        self.random ^= self.random >> 17;
        self.random ^= self.random << 5;
        self.random as f32 / u32::MAX as f32 * 2.0 - 1.0
    }
}

/// A band-pass filter that rings at one frequency: knocked, it sounds that frequency at about the
/// strength of the knock and dies away, the sooner the broader its band. A biquad after Robert
/// Bristow-Johnson's cookbook.
struct Resonator {
    b0: f32,
    a1: f32,
    a2: f32,
    /// The last two inputs and outputs.
    x: [f32; 2],
    y: [f32; 2],
}

impl Resonator {
    fn new(frequency: f32, sharpness: f32) -> Self {
        let omega = TAU * frequency / SAMPLE_RATE as f32;
        let alpha = omega.sin() / (2.0 * sharpness);
        let a0 = 1.0 + alpha;
        Self {
            // The cookbook's alpha / a0, without the alpha, so a knock rings at its own strength
            // however narrow the band.
            b0: 1.0 / a0,
            a1: -2.0 * omega.cos() / a0,
            a2: (1.0 - alpha) / a0,
            x: [0.0; 2],
            y: [0.0; 2],
        }
    }

    fn ring(&mut self, input: f32) -> f32 {
        let output = self.b0 * (input - self.x[1]) - self.a1 * self.y[0] - self.a2 * self.y[1];
        self.x = [input, self.x[0]];
        self.y = [output, self.y[0]];
        output
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn play(engine: &mut Engine, seconds: f32) -> Vec<f32> {
        (0..(seconds * SAMPLE_RATE as f32) as usize)
            .map(|_| engine.sample())
            .collect()
    }

    /// Root mean square: how loud a stretch of sound is.
    fn loudness(samples: &[f32]) -> f32 {
        (samples.iter().map(|sample| sample * sample).sum::<f32>() / samples.len() as f32).sqrt()
    }

    /// Silent until the game runs it, the engine fades in and out rather than starting or stopping
    /// dead, and never leaves its range, flat out on a boost included.
    #[test]
    fn the_engine_fades_in_and_out_within_its_range() {
        let controls = Arc::new(Controls::default());
        let mut engine = Engine::new(controls.clone());
        assert!(play(&mut engine, 0.1).iter().all(|sample| *sample == 0.0));

        controls.set(Running {
            revs: 1.2,
            load: 1.0,
            level: 1.0,
        });
        let running = play(&mut engine, 1.0);
        assert!(running.iter().all(|sample| sample.abs() <= AMPLITUDE));
        let full = loudness(&running[SAMPLE_RATE as usize / 2..]);
        assert!(full > 0.03, "{full}");

        controls.set(Running::default());
        let stopping = play(&mut engine, 1.5);
        let first = loudness(&stopping[..SAMPLE_RATE as usize / 20]);
        assert!(first > 0.3 * full, "{first} against {full}");
        assert!(
            stopping
                .iter()
                .rev()
                .take(1000)
                .all(|sample| *sample == 0.0)
        );
    }

    /// The firings, whose rate is the pitch the player hears, come as fast as the revs say.
    #[test]
    fn the_engine_fires_as_fast_as_it_turns() {
        for (revs, per_second) in [(0.0, IDLE_FIRINGS), (1.0, TOP_SPEED_FIRINGS)] {
            let controls = Arc::new(Controls::default());
            controls.set(Running {
                revs,
                load: 1.0,
                level: 1.0,
            });
            let mut engine = Engine::new(controls);
            play(&mut engine, 1.0);
            let mut firings = 0;
            for _ in 0..2 * SAMPLE_RATE {
                let weak = engine.weak;
                engine.sample();
                firings += u32::from(engine.weak != weak);
            }
            let expected = 2.0 * per_second;
            assert!(
                (firings as f32 - expected).abs() < 0.02 * expected,
                "{firings} firings at revs {revs}, not {expected}"
            );
        }
    }

    /// Pressing the accelerator makes the engine work, and be heard working.
    #[test]
    fn the_engine_is_louder_under_load() {
        let at = |load| {
            let controls = Arc::new(Controls::default());
            controls.set(Running {
                revs: 1.0,
                load,
                level: 1.0,
            });
            let mut engine = Engine::new(controls);
            play(&mut engine, 1.0);
            loudness(&play(&mut engine, 1.0))
        };
        let (coasting, loaded) = (at(0.0), at(1.0));
        assert!(loaded > 1.5 * coasting, "{loaded} against {coasting}");
    }
}
