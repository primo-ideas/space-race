//! Race rules shared by the server, which decides, and the client, which displays: how far a car
//! has driven, which lap it is on, how cars rank, and how well each one started.

use std::cmp::Ordering;

use bitcode::{Decode, Encode};
use glam::Vec2;

use super::TICK_SECONDS;
use super::car::{Car, CarInput, CarTuning};
use super::track::Track;

/// The start lights shown over the grid: one more lights up every second, and the last one lights
/// up at the start.
pub const START_LIGHTS: u32 = 4;

/// Seconds between the first start light and the start.
pub const START_LIGHTS_SECONDS: u32 = START_LIGHTS - 1;

/// How the start lights stand `until_start` seconds before the start, negative once it has passed:
/// how many are lit, and how long ago the last of them lit up. Nothing is lit while the cars wait
/// for the first one, and every light stays lit once the race is on.
pub fn start_lights(until_start: f32) -> (u32, f32) {
    let first = START_LIGHTS_SECONDS as f32;
    if until_start <= 0.0 {
        (START_LIGHTS, -until_start)
    } else if until_start <= first {
        let light = until_start.ceil();
        (START_LIGHTS - light as u32, light - until_start)
    } else {
        (0, 0.0)
    }
}

/// How far a car has driven along the track, counting every lap: the start line is at 0, one lap
/// later is at the track length.
///
/// Measured from changes of the car's projection on the centerline, so driving backward counts
/// against it and nothing can be gained by crossing the start line back and forth.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Progress {
    /// Distance along the centerline at the last update, in `[0, length)`.
    distance: f32,
    total: f32,
}

impl Progress {
    /// Starts counting at `position`. A car within half a lap behind the start line, like a car on
    /// the grid, starts below zero.
    pub fn new(track: &Track, position: Vec2) -> Self {
        let distance = track.project(position).distance;
        let total = if distance > track.length() / 2.0 {
            distance - track.length()
        } else {
            distance
        };
        Self { distance, total }
    }

    pub fn update(&mut self, track: &Track, position: Vec2) {
        let distance = track.project(position).distance;
        self.total += wrapped_delta(distance - self.distance, track.length());
        self.distance = distance;
    }

    /// Meters driven since the start line, negative before crossing it.
    pub fn total(&self) -> f32 {
        self.total
    }
}

/// A change of distance along the loop, taking the shorter way around the start line.
pub fn wrapped_delta(delta: f32, length: f32) -> f32 {
    (delta + length / 2.0).rem_euclid(length) - length / 2.0
}

/// The lap a car is on, from its [`Progress::total`]: 0 before crossing the start line, then 1,
/// 2 and so on. A car that finished a race of `n` laps is on lap `n + 1`.
pub fn lap(progress: f32, track_length: f32) -> u32 {
    if progress < 0.0 {
        0
    } else {
        (progress / track_length) as u32 + 1
    }
}

/// Where a car stands in a race, for ranking.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum RaceStanding {
    /// Crossed the finish line after `time_ms` milliseconds of racing.
    Finished { time_ms: u32 },
    /// Still racing, `progress` meters from the start line.
    Racing { progress: f32 },
}

impl RaceStanding {
    /// Ranking order: finished cars first, fastest first, then racing cars, furthest first.
    pub fn rank(&self, other: &Self) -> Ordering {
        match (self, other) {
            (Self::Finished { time_ms: a }, Self::Finished { time_ms: b }) => a.cmp(b),
            (Self::Finished { .. }, Self::Racing { .. }) => Ordering::Less,
            (Self::Racing { .. }, Self::Finished { .. }) => Ordering::Greater,
            (Self::Racing { progress: a }, Self::Racing { progress: b }) => b.total_cmp(a),
        }
    }
}

/// How close to the start a racer pressed the accelerator, from S, the best, to E. Each grade gives
/// a share of the start boost.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Encode, Decode)]
pub enum StartGrade {
    S,
    A,
    B,
    C,
    D,
    E,
}

impl StartGrade {
    /// Every grade but E, with how far from the start it is given, early or late, in seconds.
    const WINDOWS: [(Self, f32); 5] = [
        (Self::S, 0.05),
        (Self::A, 0.1),
        (Self::B, 0.15),
        (Self::C, 0.225),
        (Self::D, 0.3),
    ];

    /// Latest a press still earns more than E, in seconds after the start.
    pub const WINDOW: f32 = 0.3;

    /// The grade for pressing `offset` seconds after the start, negative before it.
    pub fn from_offset(offset: f32) -> Self {
        Self::WINDOWS
            .iter()
            .find(|(_, window)| offset.abs() <= *window)
            .map_or(Self::E, |(grade, _)| *grade)
    }

    /// Share of the start boost this grade gives, from 1 for S down to nothing for E.
    pub fn boost_share(self) -> f32 {
        match self {
            Self::S => 1.0,
            Self::A => 0.8,
            Self::B => 0.6,
            Self::C => 0.4,
            Self::D => 0.2,
            Self::E => 0.0,
        }
    }
}

/// Follows one racer's accelerator to grade their start.
///
/// The press that counts is the one held at the start, or the first one after it. Holding the
/// accelerator long before the start is an early press like any other, and earns an E.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct StartTiming {
    /// Tick of the press of the accelerator, while it is held.
    press: Option<f64>,
    grade: Option<StartGrade>,
}

impl StartTiming {
    /// The accelerator was pressed or let go at `tick`, fractional, as the player saw the race.
    pub fn accelerator(&mut self, pressed: bool, tick: f64) {
        self.press = pressed.then_some(tick);
    }

    /// Forgets the last grade, for a new race. A press held meanwhile still counts.
    pub fn reset(&mut self) {
        self.grade = None;
    }

    /// A start already graded, as a client learns it from the server when it starts predicting
    /// its car after the grade was decided: it must not be graded again.
    pub fn graded(grade: StartGrade) -> Self {
        Self {
            press: None,
            grade: Some(grade),
        }
    }

    pub fn grade(&self) -> Option<StartGrade> {
        self.grade
    }

    /// Grades the start once `tick` is past `start_tick`: from the press held then or made since,
    /// or E past `give_up_tick` without one. Returns the grade on the tick it is decided.
    pub fn update(&mut self, tick: u32, start_tick: u32, give_up_tick: u32) -> Option<StartGrade> {
        if self.grade.is_some() || tick <= start_tick {
            return None;
        }
        let grade = match self.press {
            Some(press) => {
                let ticks = press - f64::from(start_tick);
                StartGrade::from_offset((ticks * f64::from(TICK_SECONDS)) as f32)
            }
            None if tick > give_up_tick => StartGrade::E,
            None => return None,
        };
        self.grade = Some(grade);
        Some(grade)
    }
}

/// Past this tick, a start with no press is graded E: a press after it could not grade better.
pub fn give_up_tick(start_tick: u32) -> u32 {
    start_tick + (StartGrade::WINDOW / TICK_SECONDS).round() as u32
}

/// One tick of a racing car, by the rules both sides follow: the server, which decides it, and
/// the client, which predicts its own car with it, so the two cannot disagree about what a start
/// is worth. The start is graded on the first tick it can be and pays its boost at once; the car
/// stands on the grid until the start, and drives from the tick after it. Returns the grade on the
/// tick it is decided.
pub fn racing_tick(
    car: &mut Car,
    start: &mut StartTiming,
    input: CarInput,
    tick: u32,
    start_tick: u32,
    tuning: &CarTuning,
    track: &Track,
) -> Option<StartGrade> {
    let grade = start.update(tick, start_tick, give_up_tick(start_tick));
    if let Some(grade) = grade {
        car.boost = car.boost.max(grade.boost_share() * tuning.start_boost);
    }
    if tick > start_tick {
        car.step(input, tuning, track);
    }
    grade
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{oval, tuning};

    /// A racing car stands on the grid until the start, is graded on the first tick after it with
    /// its boost paid on that same tick, and drives from then on.
    #[test]
    fn a_racing_car_waits_for_the_start_then_is_graded_and_drives() {
        let track = Track::build(&oval(100.0, 40.0, 20.0)).unwrap();
        let tuning = tuning();
        let mut car = Car::on_grid(&track, 0);
        let grid = car.position;
        let mut start = StartTiming::default();
        let accelerate = CarInput::new(true, 0.0);
        start.accelerator(true, 100.0);
        let tick = |car: &mut Car, start: &mut StartTiming, tick: u32| {
            racing_tick(car, start, accelerate, tick, 100, &tuning, &track)
        };

        for number in 90..=100 {
            assert_eq!(tick(&mut car, &mut start, number), None);
            assert_eq!(car.position, grid);
        }
        assert_eq!(tick(&mut car, &mut start, 101), Some(StartGrade::S));
        assert!(car.boost > 0.0);
        assert_ne!(car.position, grid);
        assert_eq!(tick(&mut car, &mut start, 102), None);
    }

    #[test]
    fn a_start_without_a_press_is_given_up_once_no_press_could_grade_better() {
        assert_eq!(give_up_tick(600), 618);
        let mut start = StartTiming::default();
        assert_eq!(start.update(618, 600, give_up_tick(600)), None);
        assert_eq!(
            start.update(619, 600, give_up_tick(600)),
            Some(StartGrade::E)
        );
    }

    #[test]
    fn start_lights_come_on_one_a_second_the_last_at_the_start() {
        let lit = |until_start| start_lights(until_start).0;
        assert_eq!(lit(4.0), 0);
        assert_eq!(lit(3.01), 0);
        assert_eq!(lit(3.0), 1);
        assert_eq!(lit(2.5), 1);
        assert_eq!(lit(2.0), 2);
        assert_eq!(lit(1.0), 3);
        assert_eq!(lit(0.01), 3);
        assert_eq!(lit(0.0), START_LIGHTS);
        assert_eq!(lit(-5.0), START_LIGHTS);
        // How long the last light has been on.
        assert!((start_lights(2.25).1 - 0.75).abs() < 1e-6);
        assert!((start_lights(-0.4).1 - 0.4).abs() < 1e-6);
    }

    #[test]
    fn start_grades_follow_the_distance_to_the_start_either_way() {
        let grades = [
            0.0,
            0.04,
            -0.04,
            0.09,
            -0.14,
            0.2,
            -0.29,
            0.31,
            -2.0,
            f32::NAN,
        ]
        .map(StartGrade::from_offset);
        use StartGrade::*;
        assert_eq!(grades, [S, S, S, A, B, C, D, E, E, E]);
        assert_eq!(S.boost_share(), 1.0);
        assert_eq!(E.boost_share(), 0.0);
    }

    #[test]
    fn a_start_is_graded_from_the_press_that_counts() {
        const START: u32 = 600;
        const GIVE_UP: u32 = 640;
        let graded = |presses: &[(bool, f64)], ticks: u32| {
            let mut timing = StartTiming::default();
            for &(pressed, tick) in presses {
                timing.accelerator(pressed, tick);
            }
            (START - 10..START + ticks)
                .find_map(|tick| timing.update(tick, START, GIVE_UP))
                .map(|grade| (grade, timing.grade()))
        };
        let tick = |seconds: f64| f64::from(START) + seconds / f64::from(TICK_SECONDS);

        // Right at the start, held through it.
        assert_eq!(
            graded(&[(true, tick(-0.02))], 60),
            Some((StartGrade::S, Some(StartGrade::S)))
        );
        // Held since long before.
        assert_eq!(graded(&[(true, tick(-5.0))], 60).unwrap().0, StartGrade::E);
        // Pressed early, let go, pressed again late.
        let presses = [(true, tick(-1.0)), (false, tick(-0.5)), (true, tick(0.12))];
        assert_eq!(graded(&presses, 60).unwrap().0, StartGrade::B);
        // Never pressed: E once the give-up tick has passed, not before.
        assert_eq!(graded(&[], GIVE_UP - START), None);
        assert_eq!(graded(&[], 60).unwrap().0, StartGrade::E);
    }

    #[test]
    fn grid_slots_start_behind_the_line() {
        let track = Track::build(&oval(100.0, 40.0, 20.0)).unwrap();
        let (position, _) = track.grid_slot(3);
        let progress = Progress::new(&track, position);
        assert!(
            progress.total() < 0.0 && progress.total() > -30.0,
            "{progress:?}"
        );
        assert_eq!(lap(progress.total(), track.length()), 0);
    }

    #[test]
    fn progress_counts_laps_across_the_start_line() {
        let track = Track::build(&oval(100.0, 40.0, 20.0)).unwrap();
        let (start, _) = track.grid_slot(0);
        let mut progress = Progress::new(&track, start);
        let initial = progress.total();

        // Two full laps along the centerline, in steps of 5 m.
        let steps = (2.0 * track.length() / 5.0) as usize;
        for step in 1..=steps {
            let distance = initial + step as f32 * 5.0;
            progress.update(
                &track,
                track.point_at(distance.rem_euclid(track.length())).position,
            );
        }
        let expected = initial + steps as f32 * 5.0;
        assert!((progress.total() - expected).abs() < 0.5, "{progress:?}");
        assert_eq!(lap(progress.total(), track.length()), 2);
    }

    #[test]
    fn driving_backward_across_the_line_loses_progress() {
        let track = Track::build(&oval(100.0, 40.0, 20.0)).unwrap();
        let mut progress = Progress::new(&track, track.point_at(3.0).position);
        progress.update(&track, track.point_at(track.length() - 3.0).position);
        assert!((progress.total() + 3.0).abs() < 0.1, "{progress:?}");
        assert_eq!(lap(progress.total(), track.length()), 0);
    }

    #[test]
    fn finished_cars_rank_first_by_time_then_racing_cars_by_progress() {
        let mut standings = [
            RaceStanding::Racing { progress: 100.0 },
            RaceStanding::Finished { time_ms: 50_000 },
            RaceStanding::Racing { progress: 300.0 },
            RaceStanding::Finished { time_ms: 40_000 },
        ];
        standings.sort_by(RaceStanding::rank);
        assert_eq!(
            standings,
            [
                RaceStanding::Finished { time_ms: 40_000 },
                RaceStanding::Finished { time_ms: 50_000 },
                RaceStanding::Racing { progress: 300.0 },
                RaceStanding::Racing { progress: 100.0 },
            ]
        );
    }
}
