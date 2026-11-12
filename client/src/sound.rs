//! Sound, written rather than recorded: the game ships no audio file, so every sound is
//! synthesized here, from sine partials, a band of noise and an envelope. It stays in the public
//! domain like the rest of the game, weighs nothing, and suits a game made of plain shapes.
//!
//! Three kinds of sound:
//!
//! - **The race's signals**: a blip as each amber start light comes on and a fuller note as the
//!   green one starts the race, a chime at every lap and a chord at the finish, and a call when a
//!   ranked match is found. They follow what the player sees: the lights follow the player's own
//!   timeline while their car is predicted, and so do their sounds.
//! - **The car**: a tick as it breaks into a drift, a whoosh as a drift pays its boost, a deeper
//!   and longer one for the turbo, a soft bell as a charge of turbo fills, a thud on a wall, as
//!   hard as the hit. Under them, without a break, the drone of `sound/drone.rs`: thrusters, wind
//!   and the hiss of the drift. The player's own car is heard as it is predicted, on the frame the
//!   press is seen; a spectator hears the car they watch.
//! - **The menus**: a tick as the focus moves and a click on whatever is pressed.

mod drone;
mod tone;

use std::sync::Arc;

use bevy::audio::{AddAudioSource, GlobalVolume, Volume};
use bevy::input_focus::InputFocus;
use bevy::prelude::*;
use bevy::ui_widgets::Activate;
use star_racer_protocol::{LobbyPhase, PlayerStatus};
use star_racer_sim::TICK_RATE;
use star_racer_sim::car::Car;
use star_racer_sim::race;

use self::drone::{Controls, Drive, Drone};
use self::tone::{Noise, Tone};
use crate::lobby::CurrentLobby;
use crate::race::{RaceUpdate, RaceView};
use crate::screen::Screen;
use crate::track::CurrentTrack;
use crate::weight::FollowedPush;

/// The speed the drone takes for a car's top speed, in m/s: the shipped car's. The real one is
/// the server's to know, and a drone a few percent off is still the right drone.
const DRONE_TOP_SPEED: f32 = 58.0;
/// A boost that grew by more than this, in seconds, was just fired: a prediction corrected by a
/// tick or two is not one.
const BOOST_FIRED: f32 = 0.2;
/// A hit taking less than this from the car, in m/s, is a graze, and makes no sound; one taking
/// this much or more is as loud as a hit gets.
const HIT_FROM: f32 = 1.5;
const HIT_LOUDEST: f32 = 14.0;
/// Scraping along a wall is a hit on every frame: one thud at most this often, in seconds.
const HIT_EVERY: f64 = 0.15;

pub struct SoundPlugin {
    /// How loud everything is, from 0 to 1.
    pub volume: f32,
}

impl Plugin for SoundPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(GlobalVolume::new(Volume::Linear(self.volume)))
            .add_audio_source::<Tone>()
            .add_audio_source::<Drone>()
            .init_resource::<LitLights>()
            .init_resource::<MatchAnnounced>()
            .add_systems(Startup, add_sounds)
            .add_observer(click)
            .add_systems(Update, tick_on_focus)
            .add_systems(OnEnter(Screen::Lobby), forget_lights)
            .add_systems(OnExit(Screen::Lobby), silence_drone)
            .add_systems(
                Update,
                (announce_match, play_start_lights, play_laps)
                    .after(RaceUpdate)
                    .run_if(in_state(Screen::Lobby)),
            )
            // After the cars were moved and what pushed them worked out, as the rumble is.
            .add_systems(PostUpdate, play_car.run_if(in_state(Screen::Lobby)));
    }
}

#[derive(Resource)]
struct Sounds {
    /// An amber start light coming on.
    light: Handle<Tone>,
    /// The green one: the race is on.
    start: Handle<Tone>,
    /// A ranked match was found.
    found: Handle<Tone>,
    /// The player crossed the line onto another lap, and onto the end of the race.
    lap: Handle<Tone>,
    finish: Handle<Tone>,
    /// The car broke into a drift.
    drift: Handle<Tone>,
    /// A boost fired: a drift's or a start's.
    boost: Handle<Tone>,
    /// The turbo fired, and a charge of it filled.
    turbo: Handle<Tone>,
    charged: Handle<Tone>,
    /// The car hit a wall.
    hit: Handle<Tone>,
    /// In the menus: the focus moved, something was pressed.
    focus: Handle<Tone>,
    press: Handle<Tone>,
    /// What the drone is asked to sound like.
    drone: Arc<Controls>,
}

impl Sounds {
    fn play(&self, commands: &mut Commands, sound: &Handle<Tone>, volume: f32) {
        commands.spawn((
            AudioPlayer(sound.clone()),
            PlaybackSettings::DESPAWN.with_volume(Volume::Linear(volume)),
        ));
    }
}

fn add_sounds(
    mut commands: Commands,
    mut tones: ResMut<Assets<Tone>>,
    mut drones: ResMut<Assets<Drone>>,
) {
    // Low and round rather than sharp: a blip on G4 with the octave under it and barely any
    // harmonic above, then the start a fourth higher, longer and warmer, sitting on its lower
    // octave. A slower attack takes the edge off both.
    let light = tones.add(Tone {
        partials: vec![(196.0, 0.4), (392.0, 1.0), (784.0, 0.06)],
        seconds: 0.24,
        attack: 0.014,
        decay: 15.0,
        ..default()
    });
    let start = tones.add(Tone {
        partials: vec![(261.63, 0.55), (523.25, 1.0), (1046.5, 0.05)],
        seconds: 1.3,
        attack: 0.018,
        decay: 3.2,
        ..default()
    });
    // Two notes rising a fifth, soft: it calls the player back without startling them.
    let found = tones.add(Tone {
        partials: vec![(392.0, 1.0), (587.33, 0.7), (784.0, 0.12)],
        seconds: 0.7,
        attack: 0.02,
        decay: 5.0,
        ..default()
    });
    // A lap: E5 and the fifth over it, short and clear. The finish: the chord of the start, an
    // octave up and left to ring.
    let lap = tones.add(Tone {
        partials: vec![(659.25, 1.0), (987.77, 0.5), (1318.5, 0.08)],
        seconds: 0.5,
        attack: 0.012,
        decay: 7.0,
        ..default()
    });
    let finish = tones.add(Tone {
        partials: vec![(523.25, 1.0), (659.25, 0.7), (783.99, 0.6), (1046.5, 0.35)],
        seconds: 1.8,
        attack: 0.02,
        decay: 2.2,
        ..default()
    });
    // Breaking into a drift: a breath of noise falling away, the car letting go of the road.
    let drift = tones.add(Tone {
        seconds: 0.22,
        attack: 0.008,
        decay: 13.0,
        noise: Some(Noise {
            level: 0.9,
            from: 2600.0,
            to: 1100.0,
            width: 0.5,
        }),
        ..default()
    });
    // A boost: air rushing up through the spectrum over a note that climbs an octave.
    let boost = tones.add(Tone {
        partials: vec![(110.0, 0.7), (220.0, 0.35)],
        seconds: 0.7,
        attack: 0.025,
        decay: 4.0,
        slide: 2.0,
        noise: Some(Noise {
            level: 1.1,
            from: 500.0,
            to: 3200.0,
            width: 0.6,
        }),
    });
    // The turbo: the same gesture from further down, wider, longer and with a kick under it.
    let turbo = tones.add(Tone {
        partials: vec![(55.0, 1.0), (110.0, 0.6), (165.0, 0.3)],
        seconds: 1.3,
        attack: 0.012,
        decay: 2.4,
        slide: 3.0,
        noise: Some(Noise {
            level: 1.4,
            from: 300.0,
            to: 4800.0,
            width: 0.8,
        }),
    });
    // A charge of turbo filled: a small bell, two partials a fifth apart, out of the way.
    let charged = tones.add(Tone {
        partials: vec![(1174.7, 1.0), (1760.0, 0.5)],
        seconds: 0.4,
        attack: 0.006,
        decay: 10.0,
        ..default()
    });
    // A wall: a dull knock falling in pitch, with the grit of the scrape in it.
    let hit = tones.add(Tone {
        partials: vec![(85.0, 1.0), (130.0, 0.5)],
        seconds: 0.28,
        attack: 0.003,
        decay: 15.0,
        slide: 0.5,
        noise: Some(Noise {
            level: 1.2,
            from: 900.0,
            to: 220.0,
            width: 0.9,
        }),
    });
    // The menus: a tick for the focus, barely there, and a short rising click for a press.
    let focus = tones.add(Tone {
        partials: vec![(1046.5, 1.0)],
        seconds: 0.05,
        attack: 0.004,
        decay: 60.0,
        ..default()
    });
    let press = tones.add(Tone {
        partials: vec![(523.25, 1.0), (1046.5, 0.3)],
        seconds: 0.12,
        attack: 0.005,
        decay: 28.0,
        slide: 1.5,
        ..default()
    });

    // The drone plays from here to the end of the game, silent until a car asks otherwise.
    let controls = Arc::new(Controls::default());
    commands.spawn((
        AudioPlayer(drones.add(Drone(controls.clone()))),
        PlaybackSettings::ONCE,
    ));
    commands.insert_resource(Sounds {
        light,
        start,
        found,
        lap,
        finish,
        drift,
        boost,
        turbo,
        charged,
        hit,
        focus,
        press,
        drone: controls,
    });
}

/// Whatever is pressed, in any menu, with any device: they all end in an [`Activate`].
fn click(_: On<Activate>, mut commands: Commands, sounds: Res<Sounds>) {
    sounds.play(&mut commands, &sounds.press, 0.5);
}

/// A tick as the focus moves from one thing to another. Not as it first lands somewhere: opening
/// a screen is not a move.
fn tick_on_focus(
    mut commands: Commands,
    focus: Res<InputFocus>,
    sounds: Res<Sounds>,
    mut last: Local<Option<Entity>>,
) {
    let now = focus.get();
    if let (Some(from), Some(to)) = (*last, now)
        && from != to
    {
        sounds.play(&mut commands, &sounds.focus, 0.35);
    }
    *last = now;
}

/// The start lights heard so far, so each one is heard once.
#[derive(Resource, Default)]
struct LitLights(u32);

fn forget_lights(mut lit: ResMut<LitLights>, mut announced: ResMut<MatchAnnounced>) {
    lit.0 = 0;
    announced.0 = false;
}

fn silence_drone(sounds: Res<Sounds>) {
    sounds.drone.set(Drive::default());
}

/// Whether the match the player is in has been announced.
#[derive(Resource, Default)]
struct MatchAnnounced(bool);

/// Plays a sound as the player lands in a ranked match: they were waiting in the queue, and may
/// not be looking at the screen.
fn announce_match(
    mut commands: Commands,
    current: Res<CurrentLobby>,
    sounds: Res<Sounds>,
    mut announced: ResMut<MatchAnnounced>,
) {
    let ranked = current
        .0
        .as_ref()
        .and_then(|membership| membership.state.as_ref())
        .is_some_and(|state| state.ranked);
    if ranked && !announced.0 {
        announced.0 = true;
        sounds.play(&mut commands, &sounds.found, 1.0);
    }
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
    // On the player's own timeline while their car is predicted, as the lights are drawn: on the
    // displayed race, the sound of the start came a lead and three ticks after its light.
    let lit = match (phase, view.own_tick.or(view.tick)) {
        (Some(LobbyPhase::Racing { start_tick }), Some(tick)) => {
            let until_start = (f64::from(*start_tick) - tick) / f64::from(TICK_RATE);
            race::start_lights(until_start as f32).0
        }
        _ => 0,
    };

    if lit > played.0 {
        // However many lights were missed, only the one that just came on is heard.
        let start = lit == race::START_LIGHTS;
        let sound = if start { &sounds.start } else { &sounds.light };
        sounds.play(&mut commands, sound, 1.0);
        debug!(light = lit, start, "start light sound");
    }
    played.0 = lit;
}

/// Where the player's race stood on the last frame: the lap they were on, if they were racing,
/// and whether they had finished.
#[derive(Default)]
struct RaceHeard {
    lap: Option<u32>,
    finished: bool,
}

/// A chime as the player starts another lap, and a chord as they finish the race.
fn play_laps(
    mut commands: Commands,
    current: Res<CurrentLobby>,
    view: Res<RaceView>,
    track: Option<Res<CurrentTrack>>,
    sounds: Res<Sounds>,
    mut heard: Local<RaceHeard>,
) {
    let (Some(membership), Some(track)) = (&current.0, track) else {
        *heard = RaceHeard::default();
        return;
    };
    let status = membership.own_status();
    let finished = matches!(status, Some(PlayerStatus::Finished { .. }));
    let lap = (status == Some(PlayerStatus::Racing))
        .then(|| view.cars.iter().find(|car| car.id == membership.car))
        .flatten()
        .map(|car| race::lap(car.progress, track.0.length()));

    if finished && !heard.finished {
        sounds.play(&mut commands, &sounds.finish, 1.0);
    } else if let (Some(before), Some(now)) = (heard.lap, lap)
        && lap_begun(before, now, u32::from(membership.settings.laps))
    {
        sounds.play(&mut commands, &sounds.lap, 0.8);
    }
    *heard = RaceHeard { lap, finished };
}

/// Whether going from lap `before` to lap `now` begins a lap worth a chime, in a race of `laps`:
/// not the first, which the start lights announce, and not the one past the last, which is the
/// finish and has its own sound.
fn lap_begun(before: u32, now: u32, laps: u32) -> bool {
    now > before && now >= 2 && now <= laps
}

/// What the heard car was doing on the last frame.
#[derive(Default)]
struct CarHeard {
    /// Whose car it was: a spectator who changes car hears no event for the change.
    id: Option<star_racer_protocol::CarId>,
    car: Car,
    hit: f64,
}

/// The car's own sounds, and what the drone is asked for: the player's car as it is predicted,
/// or the car a spectator watches.
fn play_car(
    mut commands: Commands,
    view: Res<RaceView>,
    push: Res<FollowedPush>,
    time: Res<Time<Real>>,
    sounds: Res<Sounds>,
    mut heard: Local<CarHeard>,
) {
    let Some((id, followed)) = view.followed else {
        sounds.drone.set(Drive::default());
        *heard = CarHeard::default();
        return;
    };
    // The player's own car is ahead of its snapshot: heard as it is drawn.
    let car = view.own_car.unwrap_or(followed);
    let charge = view
        .cars
        .iter()
        .find(|snapshot| snapshot.id == id)
        .map_or(0.0, |snapshot| snapshot.drift_gauge);
    sounds.drone.set(Drive {
        on: 1.0,
        speed: car.velocity.length() / DRONE_TOP_SPEED,
        boost: if car.turbo_boost > 0.0 {
            1.0
        } else if car.boost > 0.0 {
            0.6
        } else {
            0.0
        },
        drift: f32::from(u8::from(car.is_drifting())),
        charge,
    });

    if heard.id == Some(id) {
        let before = &heard.car;
        if car.is_drifting() && !before.is_drifting() {
            sounds.play(&mut commands, &sounds.drift, 0.6);
        }
        if car.turbo_boost > before.turbo_boost + BOOST_FIRED {
            sounds.play(&mut commands, &sounds.turbo, 1.0);
        } else if car.boost > before.boost + BOOST_FIRED {
            // A long boost is a loud one.
            sounds.play(
                &mut commands,
                &sounds.boost,
                0.55 + 0.35 * car.boost.min(1.3),
            );
        }
        if car.turbo.floor() > before.turbo.floor() && car.turbo_boost <= before.turbo_boost {
            sounds.play(&mut commands, &sounds.charged, 0.6);
        }
        let now = time.elapsed_secs_f64();
        if let Some(push) = push.0
            && let Some(volume) = hit_volume(push.impact)
            && now - heard.hit >= HIT_EVERY
        {
            heard.hit = now;
            sounds.play(&mut commands, &sounds.hit, volume);
        }
    }
    heard.id = Some(id);
    heard.car = car;
}

/// How loud a hit that took `impact` m/s from the car is, from a tap to a full blow, if it is more
/// than a graze.
fn hit_volume(impact: f32) -> Option<f32> {
    (impact >= HIT_FROM).then(|| (impact / HIT_LOUDEST).clamp(0.3, 1.0))
}

#[cfg(test)]
mod tests {
    use bevy::audio::Decodable;

    use super::*;

    #[test]
    fn a_lap_chimes_from_the_second_to_the_last() {
        assert!(!lap_begun(0, 1, 3), "the start lights said it");
        assert!(lap_begun(1, 2, 3));
        assert!(lap_begun(2, 3, 3));
        assert!(!lap_begun(3, 4, 3), "that is the finish");
        assert!(!lap_begun(2, 2, 3));
        assert!(!lap_begun(1, 2, 1));
    }

    #[test]
    fn a_hit_is_as_loud_as_it_was_hard() {
        assert_eq!(hit_volume(1.0), None);
        let (tap, blow) = (hit_volume(2.0).unwrap(), hit_volume(30.0).unwrap());
        assert!(tap < blow && blow == 1.0);
    }

    /// Every sound the game plays starts and ends at silence, is heard, and never clips.
    #[test]
    fn every_sound_is_heard_and_clean() {
        let mut app = App::new();
        app.init_resource::<Assets<Tone>>()
            .init_resource::<Assets<Drone>>()
            .add_systems(Startup, add_sounds);
        app.update();
        let tones = app.world().resource::<Assets<Tone>>();
        assert_eq!(tones.len(), 12);
        for (_, tone) in tones.iter() {
            let samples: Vec<f32> = tone.decoder().collect();
            let peak = samples
                .iter()
                .fold(0.0f32, |peak, sample| peak.max(sample.abs()));
            assert_eq!(samples[0], 0.0, "{tone:?}");
            assert!(samples.last().unwrap().abs() < 2e-3, "{tone:?}");
            assert!(peak > 0.08 && peak < 0.95, "{peak}: {tone:?}");
        }
    }
}
