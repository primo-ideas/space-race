//! A short synthetic sound, played once: sine partials whose pitch may slide, and a band of noise
//! that may sweep, under one envelope. A blip, a chime, a whoosh and a thud are all one of these.

use std::f32::consts::{PI, TAU};
use std::num::NonZero;
use std::time::Duration;

use bevy::audio::{ChannelCount, Decodable, Sample, SampleRate, Source};
use bevy::prelude::*;

/// Samples a second, the usual rate for sound.
pub const SAMPLE_RATE: u32 = 44_100;
/// How loud a tone is at its fullest, leaving room for several at once without clipping.
const AMPLITUDE: f32 = 0.35;
/// Seconds a tone takes to fall silent at its end, so it stops without a click.
const RELEASE: f32 = 0.02;
/// The highest a filter is tuned, in Hz: the simple filter used here goes wild near a sixth of the
/// sample rate.
const FILTER_CEILING: f32 = 6_000.0;

#[derive(Asset, TypePath, Clone, Debug)]
pub struct Tone {
    /// Frequency in Hz and share of the volume, one pair per partial. A single partial is a plain
    /// sine; adding multiples of its frequency gives the sound a body.
    pub partials: Vec<(f32, f32)>,
    /// How long it lasts, in seconds.
    pub seconds: f32,
    /// Seconds to reach full volume. Short, but not zero: an instant start clicks.
    pub attack: f32,
    /// How quickly it dies away afterward, per second.
    pub decay: f32,
    /// What the pitch of the partials is multiplied by when the tone ends, sliding there all the
    /// way through: 2 rises an octave, 0.5 falls one, 1 holds the note.
    pub slide: f32,
    /// A band of noise under the same envelope: the breath of a whoosh, the grit of a thud.
    pub noise: Option<Noise>,
}

impl Default for Tone {
    fn default() -> Self {
        Self {
            partials: Vec::new(),
            seconds: 0.2,
            attack: 0.01,
            decay: 10.0,
            slide: 1.0,
            noise: None,
        }
    }
}

/// Noise heard through a band that sweeps from one frequency to another over the tone.
#[derive(Clone, Copy, Debug)]
pub struct Noise {
    /// How loud, beside partials whose shares add up to about 1.
    pub level: f32,
    /// Where the band sits as the tone starts and as it ends, in Hz.
    pub from: f32,
    pub to: f32,
    /// How wide the band is: 1 is a broad hiss, 0.2 a narrow, whistling one.
    pub width: f32,
}

impl Decodable for Tone {
    type Decoder = ToneSamples;

    fn decoder(&self) -> Self::Decoder {
        ToneSamples {
            phases: vec![0.0; self.partials.len()],
            tone: self.clone(),
            sample: 0,
            samples: (self.seconds * SAMPLE_RATE as f32) as u32,
            hiss: Hiss::new(0x5eed),
            filter: Filter::default(),
        }
    }
}

/// A [`Tone`] played out sample by sample.
pub struct ToneSamples {
    tone: Tone,
    /// Where each partial is in its cycle, from 0 to 1: kept rather than worked out from the time,
    /// so a sliding pitch stays one smooth wave.
    phases: Vec<f32>,
    sample: u32,
    samples: u32,
    hiss: Hiss,
    filter: Filter,
}

impl Iterator for ToneSamples {
    type Item = Sample;

    fn next(&mut self) -> Option<Sample> {
        if self.sample >= self.samples {
            return None;
        }
        let seconds = self.sample as f32 / SAMPLE_RATE as f32;
        let progress = seconds / self.tone.seconds;
        self.sample += 1;

        let attack = (seconds / self.tone.attack).min(1.0);
        let release = ((self.tone.seconds - seconds) / RELEASE).clamp(0.0, 1.0);
        let envelope = attack * release * (-self.tone.decay * seconds).exp();

        let slide = self.tone.slide.powf(progress);
        let mut wave = 0.0;
        for ((frequency, share), phase) in self.tone.partials.iter().zip(&mut self.phases) {
            wave += share * (TAU * *phase).sin();
            *phase = (*phase + frequency * slide / SAMPLE_RATE as f32).fract();
        }
        if let Some(noise) = self.tone.noise {
            let band = noise.from * (noise.to / noise.from).powf(progress);
            let (_, heard) = self.filter.run(self.hiss.next(), band, noise.width);
            wave += noise.level * heard;
        }
        Some((AMPLITUDE * envelope * wave).clamp(-1.0, 1.0))
    }
}

impl Source for ToneSamples {
    /// The sound never changes rate or channel count.
    fn current_span_len(&self) -> Option<usize> {
        None
    }

    fn channels(&self) -> ChannelCount {
        NonZero::new(1).expect("one channel")
    }

    fn sample_rate(&self) -> SampleRate {
        NonZero::new(SAMPLE_RATE).expect("a sample rate above zero")
    }

    fn total_duration(&self) -> Option<Duration> {
        Some(Duration::from_secs_f32(self.tone.seconds))
    }
}

/// White noise from a seed: the same hiss on every machine, and no dependency to make it.
#[derive(Clone, Copy, Debug)]
pub struct Hiss(u32);

impl Hiss {
    pub fn new(seed: u32) -> Self {
        Self(seed)
    }

    /// The next sample, from -1 to 1.
    pub fn next(&mut self) -> f32 {
        self.0 = self.0.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        (self.0 >> 8) as f32 / 8_388_608.0 - 1.0
    }
}

/// A filter that can be retuned on every sample, which is what a sweep needs: it gives what lies
/// below a frequency and what lies around it.
#[derive(Clone, Copy, Debug, Default)]
pub struct Filter {
    low: f32,
    band: f32,
}

impl Filter {
    /// Passes one sample through the filter tuned to `hertz`, and returns what is below that
    /// frequency and what is around it. `width` is how wide "around" is, from about 0.1, narrow
    /// and ringing, to 1.5, broad.
    pub fn run(&mut self, input: f32, hertz: f32, width: f32) -> (f32, f32) {
        let tuning = 2.0 * (PI * hertz.clamp(20.0, FILTER_CEILING) / SAMPLE_RATE as f32).sin();
        self.low += tuning * self.band;
        let high = input - self.low - width * self.band;
        self.band += tuning * high;
        (self.low, self.band)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A tone must start and end at silence, or it clicks, and stay within the range the sound card
    /// takes.
    #[test]
    fn a_tone_starts_and_ends_silently_without_clipping() {
        let tone = Tone {
            partials: vec![(880.0, 1.0), (1760.0, 0.2)],
            seconds: 0.2,
            attack: 0.004,
            decay: 20.0,
            ..default()
        };
        let samples: Vec<f32> = tone.decoder().collect();
        assert_eq!(samples.len(), (0.2 * SAMPLE_RATE as f32) as usize);
        assert_eq!(samples[0], 0.0);
        assert!(samples.last().unwrap().abs() < 1e-3, "{:?}", samples.last());
        assert!(samples.iter().all(|sample| sample.abs() <= 1.0));
        let loudest = samples
            .iter()
            .fold(0.0f32, |peak, sample| peak.max(sample.abs()));
        assert!(loudest > 0.1, "{loudest}");
    }

    /// How many times a second the wave crosses zero going up, over a stretch of it.
    fn pitch(samples: &[f32]) -> f32 {
        let crossings = samples
            .windows(2)
            .filter(|pair| pair[0] < 0.0 && pair[1] >= 0.0)
            .count();
        crossings as f32 * SAMPLE_RATE as f32 / samples.len() as f32
    }

    /// A sliding tone ends on the pitch it slides to, in one smooth wave.
    #[test]
    fn a_slide_carries_the_pitch_up() {
        let tone = Tone {
            partials: vec![(200.0, 1.0)],
            seconds: 1.0,
            decay: 0.0,
            slide: 2.0,
            ..default()
        };
        let samples: Vec<f32> = tone.decoder().collect();
        let tenth = samples.len() / 10;
        let (first, last) = (
            pitch(&samples[..tenth]),
            pitch(&samples[samples.len() - tenth..]),
        );
        assert!((first - 207.0).abs() < 12.0, "{first}");
        assert!((last - 386.0).abs() < 16.0, "{last}");
        // No jump from one sample to the next beyond what the highest pitch makes.
        let steepest = samples
            .windows(2)
            .fold(0.0f32, |most, pair| most.max((pair[1] - pair[0]).abs()));
        assert!(steepest < 0.03, "{steepest}");
    }

    /// Noise through the filter is heard, stays in range, and is brighter tuned high than low.
    #[test]
    fn a_band_of_noise_follows_its_tuning() {
        let noise = |from: f32, to: f32| Tone {
            seconds: 0.5,
            decay: 0.0,
            noise: Some(Noise {
                level: 1.0,
                from,
                to,
                width: 0.5,
            }),
            ..default()
        };
        let low: Vec<f32> = noise(300.0, 300.0).decoder().collect();
        let high: Vec<f32> = noise(3000.0, 3000.0).decoder().collect();
        assert!(low.iter().chain(&high).all(|sample| sample.abs() <= 1.0));
        // A band is not a note: its crossings only say that one sits well above the other.
        let (dark, bright) = (pitch(&low), pitch(&high));
        assert!(bright > 2.0 * dark, "{dark} {bright}");
        let loudness = |samples: &[f32]| {
            (samples.iter().map(|sample| sample * sample).sum::<f32>() / samples.len() as f32)
                .sqrt()
        };
        assert!(loudness(&low) > 0.02, "{}", loudness(&low));
    }
}
