//! Game data read from the data directory: tracks and car tuning.
//!
//! ```text
//! <data dir>/
//!   car.ron            car tuning, reloaded while the server runs
//!   tracks/<key>.ron   one file per track, named by its key
//! ```

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use anyhow::{Context, Result, bail};
use serde::de::DeserializeOwned;
use space_race_sim::car::CarTuning;
use space_race_sim::track::{Track, TrackDescription};
use tokio::sync::watch;
use tracing::{info, warn};

pub const CAR_TUNING_FILE: &str = "car.ron";
pub const TRACKS_DIR: &str = "tracks";

/// How often the car tuning file is checked for changes.
const TUNING_CHECK_INTERVAL: Duration = Duration::from_secs(1);

/// Every track lobbies can race on, by key.
pub type Tracks = BTreeMap<String, Arc<LoadedTrack>>;

/// A track as written in its file, and the geometry built from it.
pub struct LoadedTrack {
    pub description: TrackDescription,
    pub track: Track,
}

/// Loads every `.ron` file of the tracks directory. A server without tracks has nothing to race on,
/// so that is an error.
pub fn load_tracks(data_dir: &Path) -> Result<Tracks> {
    let dir = data_dir.join(TRACKS_DIR);
    let entries = fs::read_dir(&dir).with_context(|| format!("cannot read {}", dir.display()))?;
    let mut tracks = Tracks::new();
    for entry in entries {
        let path = entry?.path();
        if path.extension().is_none_or(|extension| extension != "ron") {
            continue;
        }
        let Some(key) = path.file_stem().and_then(|stem| stem.to_str()) else {
            bail!("track file name is not valid UTF-8: {}", path.display());
        };
        tracks.insert(key.to_owned(), Arc::new(load_track(&path)?));
    }
    if tracks.is_empty() {
        bail!("no track in {}", dir.display());
    }
    Ok(tracks)
}

pub fn load_track(path: &Path) -> Result<LoadedTrack> {
    let description: TrackDescription = read_ron(path)?;
    let track =
        Track::build(&description).with_context(|| format!("invalid track {}", path.display()))?;
    Ok(LoadedTrack { description, track })
}

/// Loads the car tuning, which must suit every track.
pub fn load_car_tuning(path: &Path, tracks: &Tracks) -> Result<CarTuning> {
    let tuning: CarTuning = read_ron(path)?;
    tuning
        .validate()
        .map_err(anyhow::Error::msg)
        .with_context(|| format!("invalid car tuning {}", path.display()))?;
    for (key, loaded) in tracks {
        let track = &loaded.track;
        // The car has to fit through the bottlenecks, not just along the open road.
        if tuning.collision_radius >= track.narrowest_half_width() {
            bail!(
                "invalid car tuning {}: a collision radius of {} m does not fit on track {key}, \
                 {} m wide where it pinches in",
                path.display(),
                tuning.collision_radius,
                track.narrowest_half_width() * 2.0
            );
        }
    }
    Ok(tuning)
}

/// Watches the car tuning file, so the feel of the car can be adjusted while driving.
pub struct CarTuningFile {
    path: PathBuf,
    modified: Option<SystemTime>,
    tracks: Tracks,
}

impl CarTuningFile {
    pub fn load(path: PathBuf, tracks: &Tracks) -> Result<(Self, CarTuning)> {
        let modified = modified_time(&path);
        let tuning = load_car_tuning(&path, tracks)?;
        let file = Self {
            path,
            modified,
            tracks: tracks.clone(),
        };
        Ok((file, tuning))
    }

    /// Checks the file every second on a task of its own, and publishes every valid change. Every
    /// lobby follows the receiver.
    pub fn watch(mut self, tuning: CarTuning) -> watch::Receiver<CarTuning> {
        let (sender, receiver) = watch::channel(tuning);
        tokio::spawn(async move {
            let mut checks = tokio::time::interval(TUNING_CHECK_INTERVAL);
            loop {
                checks.tick().await;
                if let Some(tuning) = self.reload_if_changed() {
                    sender.send_replace(tuning);
                }
            }
        });
        receiver
    }

    /// The new tuning if the file changed since the last check and is valid. An invalid file is
    /// reported and ignored, so a typo while tuning never stops a race.
    fn reload_if_changed(&mut self) -> Option<CarTuning> {
        let modified = modified_time(&self.path);
        if modified == self.modified {
            return None;
        }
        self.modified = modified;

        match load_car_tuning(&self.path, &self.tracks) {
            Ok(tuning) => {
                info!(path = %self.path.display(), "car tuning reloaded");
                Some(tuning)
            }
            Err(error) => {
                warn!("car tuning not reloaded, keeping the previous one: {error:#}");
                None
            }
        }
    }
}

fn modified_time(path: &Path) -> Option<SystemTime> {
    fs::metadata(path)
        .and_then(|metadata| metadata.modified())
        .ok()
}

fn read_ron<T: DeserializeOwned>(path: &Path) -> Result<T> {
    let text =
        fs::read_to_string(path).with_context(|| format!("cannot read {}", path.display()))?;
    ron::from_str(&text).with_context(|| format!("cannot parse {}", path.display()))
}

#[cfg(test)]
mod tests {
    use space_race_sim::autopilot::{self, Style};
    use space_race_sim::track::scenery::Prop;

    use super::*;

    fn data_dir() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("data")
    }

    /// Every shipped track loads, and the autopilot laps it with the shipped car: drifting without
    /// touching a wall and faster than gripping, and cleanly in some style when a few ticks late. A
    /// track or tuning edit that makes a circuit undrivable, or drifting pointless, fails here.
    #[test]
    fn shipped_tracks_are_drivable() {
        let tracks = load_tracks(&data_dir()).unwrap();
        let tuning = load_car_tuning(&data_dir().join(CAR_TUNING_FILE), &tracks).unwrap();
        for (key, loaded) in &tracks {
            // Driving from the live car, like the server, and a few ticks late, like a client.
            let mut lap_times = Vec::new();
            let mut clean_while_late = false;
            for style in [Style::Grip, Style::Drift] {
                for delay_ticks in [0, 6] {
                    let report =
                        autopilot::drive_lap(&loaded.track, &tuning, style, 120.0, delay_ticks);
                    let context = format!("{key}, {style:?}, {delay_ticks} ticks late");
                    println!("{context}: {report:?}");
                    let lap_time = report.lap_time.expect(&context);
                    if delay_ticks == 0 {
                        // Gripping may have to scrape through turns meant for drifting. A
                        // drifting lap gets a small budget instead of none: a slide worth having
                        // runs wide, and a bottleneck is drawn to leave it little room. A graze
                        // of a few hundredths of a second is not a circuit that cannot be driven;
                        // a lap spent against the wall is, and that is what this still catches.
                        if style == Style::Drift {
                            assert!(report.wall_ticks <= 3, "{context}: {report:?}");
                        }
                        lap_times.push(lap_time);
                    } else {
                        clean_while_late |= report.wall_ticks == 0;
                    }
                    if style == Style::Drift {
                        assert!(report.boost_ticks > 0, "{context}: {report:?}");
                    }
                }
            }
            // Late information can make a style unfit for a track, gripping through tight turns for
            // one, but a client must still have a clean way round.
            assert!(
                clean_while_late,
                "{key}: no style laps cleanly 6 ticks late"
            );
            // Drifting through the turns must get round faster: drifting has to pay.
            let [grip, drift] = lap_times[..] else {
                unreachable!();
            };
            assert!(drift < grip, "{key}: drifting {drift} s, gripping {grip} s");
        }
    }

    /// Scenery is decoration, so building a track only checks that a prop stands off the road where
    /// it is placed. A circuit doubles back on itself, though, so a stand or a slab put far out on
    /// one turn can land on another part of the lap: every shipped prop is checked against the whole
    /// centerline here. A prop is sampled along its length and out to its depth, which is rough but
    /// enough to catch one standing on a road it does not belong to.
    #[test]
    fn shipped_scenery_stands_clear_of_the_rest_of_the_lap() {
        /// How much room a prop must leave around any other stretch of road, in meters past its
        /// edge.
        const MARGIN: f32 = 3.0;
        /// How much of the lap around a prop is the road it belongs to, in meters either way.
        const OWN_ROAD: f32 = 40.0;

        let tracks = load_tracks(&data_dir()).unwrap();
        for (key, loaded) in &tracks {
            let track = &loaded.track;
            // How close the road comes to a position, leaving out the stretch the prop stands by.
            let elsewhere = |position, at: f32| {
                track
                    .points()
                    .iter()
                    .enumerate()
                    .filter(|(index, _)| {
                        let distance = *index as f32 * track.spacing();
                        let gap = (distance - at.rem_euclid(track.length())).abs();
                        gap.min(track.length() - gap) > OWN_ROAD
                    })
                    .map(|(_, point)| point.position.distance(position))
                    .fold(f32::INFINITY, f32::min)
            };

            for (index, prop) in loaded.description.scenery.iter().enumerate() {
                // A gantry spans the road on purpose; every other prop stands beside it.
                let Some(side) = prop.side() else {
                    continue;
                };
                let (length, depth) = footprint(prop);
                let offset = prop.offset().unwrap_or_default();
                let mut closest = f32::INFINITY;
                for step in 0..=4 {
                    let at = prop.at() + length * step as f32 / 4.0;
                    let point = track.point_at(at);
                    for out in [0.0, depth / 2.0, depth] {
                        let lateral = side.sign() * (point.half_width + offset + out);
                        let position = point.position + point.direction.perp() * lateral;
                        closest = closest.min(elsewhere(position, at));
                    }
                }
                println!("{key}: prop {index} comes {closest:.0} m from the rest of the lap");
                assert!(
                    closest > track.half_width() + MARGIN,
                    "{key}: prop {index} ({prop:?}) stands {closest:.1} m from another part of the \
                     road, which is {:.0} m wide",
                    track.half_width() * 2.0
                );
            }
        }
    }

    /// Roughly how far a prop runs along the road and how far it reaches away from it, in meters.
    fn footprint(prop: &Prop) -> (f32, f32) {
        match *prop {
            Prop::Grandstand { length, rows, .. } => (length, rows as f32 * 2.0),
            Prop::Billboard { width, .. } => (width, 1.0),
            Prop::Monolith { width, .. } => (width, width * 0.5),
            _ => (0.0, 3.0),
        }
    }

    #[test]
    fn typos_in_track_files_are_errors() {
        let text = "(name: \"Typo\", width: 16.0, segments: [Turn(angle: 360.0, radius: 40.0, \
                    transition: 10.0)])";
        assert!(ron::from_str::<TrackDescription>(text).is_ok());

        let typo = text.replace("transition", "transtion");
        let error = ron::from_str::<TrackDescription>(&typo).unwrap_err();
        assert!(error.to_string().contains("transtion"), "{error}");
    }
}
