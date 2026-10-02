//! Sound, written rather than recorded: the game ships no audio file, so every sound is a few sine
//! partials under an envelope, synthesized here. It stays in the public domain like the rest of the
//! game, weighs nothing, and suits a game made of plain shapes.
//!
//! The start lights are the first sounds: a short blip as each amber light comes on, and a longer,
//! fuller note as the green one starts the race. They follow the lights, which follow the race as
//! displayed, so what the player hears and what they see agree.

use std::f32::consts::TAU;
use std::num::NonZero;
use std::time::Duration;

use bevy::audio::{
    AddAudioSource, ChannelCount, Decodable, GlobalVolume, Sample, SampleRate, Source, Volume,
};
use bevy::prelude::*;
use space_race_protocol::LobbyPhase;
use space_race_sim::TICK_RATE;
use space_race_sim::race;

use crate::lobby::CurrentLobby;
use crate::race::{RaceUpdate, RaceView};
use crate::screen::Screen;

/// Samples a second, the usual rate for sound.
const SAMPLE_RATE: u32 = 44_100;
/// How loud a tone is at its fullest, leaving room for several at once without clipping.
const AMPLITUDE: f32 = 0.35;
/// Seconds a tone takes to fall silent at its end, so it stops without a click.
const RELEASE: f32 = 0.02;

pub struct SoundPlugin {
    /// How loud everything is, from 0 to 1.
    pub volume: f32,
}

impl Plugin for SoundPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(GlobalVolume::new(Volume::Linear(self.volume)))
            .add_audio_source::<Tone>()
            .init_resource::<LitLights>()
            .add_systems(Startup, add_sounds)
            .add_systems(OnEnter(Screen::Lobby), forget_lights)
            .add_systems(
                Update,
                play_start_lights
                    .after(RaceUpdate)
                    .run_if(in_state(Screen::Lobby)),
            );
    }
}

/// A short synthetic sound: sine partials sharing one envelope.
#[derive(Asset, TypePath, Clone, Debug)]
pub struct Tone {
    /// Frequency in Hz and share of the volume, one pair per partial. A single partial is a plain
    /// sine; adding multiples of its frequency gives the sound a body.
    partials: Vec<(f32, f32)>,
    /// How long it lasts, in seconds.
    seconds: f32,
    /// Seconds to reach full volume. Short, but not zero: an instant start clicks.
    attack: f32,
    /// How quickly it dies away afterward, per second.
    decay: f32,
}

impl Decodable for Tone {
    type Decoder = ToneSamples;

    fn decoder(&self) -> Self::Decoder {
        ToneSamples {
            tone: self.clone(),
            sample: 0,
            samples: (self.seconds * SAMPLE_RATE as f32) as u32,
        }
    }
}

/// A [`Tone`] played out sample by sample.
pub struct ToneSamples {
    tone: Tone,
    sample: u32,
    samples: u32,
}

impl Iterator for ToneSamples {
    type Item = Sample;

    fn next(&mut self) -> Option<Sample> {
        if self.sample >= self.samples {
            return None;
        }
        let seconds = self.sample as f32 / SAMPLE_RATE as f32;
        self.sample += 1;

        let attack = (seconds / self.tone.attack).min(1.0);
        let release = ((self.tone.seconds - seconds) / RELEASE).clamp(0.0, 1.0);
        let envelope = attack * release * (-self.tone.decay * seconds).exp();
        let wave: f32 = self
            .tone
            .partials
            .iter()
            .map(|(frequency, share)| share * (TAU * frequency * seconds).sin())
            .sum();
        Some(AMPLITUDE * envelope * wave)
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

#[derive(Resource)]
struct Sounds {
    /// An amber start light coming on.
    light: Handle<Tone>,
    /// The green one: the race is on.
    start: Handle<Tone>,
}

fn add_sounds(mut commands: Commands, mut tones: ResMut<Assets<Tone>>) {
    // Low and round rather than sharp: a blip on G4 with the octave under it and barely any
    // harmonic above, then the start a fourth higher, longer and warmer, sitting on its lower
    // octave. A slower attack takes the edge off both.
    let light = tones.add(Tone {
        partials: vec![(196.0, 0.4), (392.0, 1.0), (784.0, 0.06)],
        seconds: 0.24,
        attack: 0.014,
        decay: 15.0,
    });
    let start = tones.add(Tone {
        partials: vec![(261.63, 0.55), (523.25, 1.0), (1046.5, 0.05)],
        seconds: 1.3,
        attack: 0.018,
        decay: 3.2,
    });
    commands.insert_resource(Sounds { light, start });
}

/// The start lights heard so far, so each one is heard once.
#[derive(Resource, Default)]
struct LitLights(u32);

fn forget_lights(mut lit: ResMut<LitLights>) {
    lit.0 = 0;
}

fn play_start_lights(
    mut commands: Commands,
    current: Res<CurrentLobby>,
    view: Res<RaceView>,
    sounds: Res<Sounds>,
    mut played: ResMut<LitLights>,
) {
    let phase = current
        .0
        .as_ref()
        .and_then(|membership| membership.state.as_ref())
        .map(|state| &state.phase);
    let lit = match (phase, view.tick) {
        (Some(LobbyPhase::Racing { start_tick }), Some(tick)) => {
            let until_start = (f64::from(*start_tick) - tick) / f64::from(TICK_RATE);
            race::start_lights(until_start as f32).0
        }
        _ => 0,
    };

    if lit > played.0 {
        // However many lights were missed, only the one that just came on is heard.
        let start = lit == race::START_LIGHTS;
        let sound = if start {
            sounds.start.clone()
        } else {
            sounds.light.clone()
        };
        commands.spawn((AudioPlayer(sound), PlaybackSettings::DESPAWN));
        debug!(light = lit, start, "start light sound");
    }
    played.0 = lit;
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
}
