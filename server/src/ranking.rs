//! The ranked ladder: every player's rating, how a race moves it, and the file it is kept in.
//!
//! Ratings are Elo, played pairwise: a race among several players counts as one duel between every
//! two of them, won by whoever finished ahead, and a player's change is their average over those
//! duels. Beating the favourite pays more than beating the outsider, and a race of eight moves a
//! rating no more than a duel does. New players move faster until they are placed.
//!
//! The file lives outside the data directory, which every deployment replaces. It is written whole
//! after every match, to a temporary file renamed over the old one, so a crash mid-write leaves
//! the previous ladder rather than half of one.

use std::collections::HashMap;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use space_race_protocol::auth::PublicKey;
use space_race_protocol::ranked::{LeaderboardEntry, PLACEMENT_GAMES, RankedProfile};
use tokio::sync::mpsc;
use tracing::{error, info, warn};

use crate::session::PlayerId;

/// Where every player starts.
pub const STARTING_RATING: i32 = 1000;

/// How far one race can move a rating, while being placed and afterwards. A player winning every
/// duel against equals gains half of it.
const K_PLACEMENT: f32 = 48.0;
const K_PLACED: f32 = 24.0;

/// A rating gap of this many points is ten to one odds.
const ODDS_SCALE: f32 = 400.0;

const LEADERBOARD_SIZE: usize = 10;

/// A player's place in a finished match: what it did to their rating.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Placed {
    pub id: PlayerId,
    pub nickname: String,
    pub change: i32,
    pub won: bool,
}

/// What a race does to each rating, for `(rating, games played)` listed in finishing order.
pub fn rating_changes(finishing_order: &[(i32, u32)]) -> Vec<i32> {
    let opponents = finishing_order.len().saturating_sub(1) as f32;
    if opponents == 0.0 {
        return vec![0; finishing_order.len()];
    }
    finishing_order
        .iter()
        .enumerate()
        .map(|(index, &(rating, games))| {
            let surprise: f32 = finishing_order
                .iter()
                .enumerate()
                .filter(|(other, _)| *other != index)
                .map(|(other, &(other_rating, _))| {
                    let expected =
                        1.0 / (1.0 + 10f32.powf((other_rating - rating) as f32 / ODDS_SCALE));
                    let scored = if index < other { 1.0 } else { 0.0 };
                    scored - expected
                })
                .sum();
            let k = if games < PLACEMENT_GAMES {
                K_PLACEMENT
            } else {
                K_PLACED
            };
            (k * surprise / opponents).round() as i32
        })
        .collect()
}

/// A player's line in the file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct Record {
    /// Their public key, in hexadecimal.
    player: String,
    /// The nickname they last played under.
    nickname: String,
    rating: i32,
    games: u32,
    wins: u32,
}

/// Every player who has played a ranked match.
pub struct Ratings {
    records: HashMap<PlayerId, Record>,
    /// Hands the file's new contents to the task that writes it. `None` keeps ratings in memory.
    file: Option<mpsc::UnboundedSender<String>>,
}

impl Ratings {
    /// A ladder that is never saved, for tests.
    #[cfg(test)]
    pub fn in_memory() -> Self {
        Self {
            records: HashMap::new(),
            file: None,
        }
    }

    /// Reads the ladder from `path`, and keeps it there from now on. No file is an empty ladder. A
    /// file that cannot be understood is moved aside rather than overwritten: the game stays up
    /// with a new ladder, and the old one can still be repaired by hand.
    pub fn load(path: PathBuf) -> Result<Self> {
        let records = match std::fs::read_to_string(&path) {
            Ok(text) => match ron::from_str::<Vec<Record>>(&text) {
                Ok(records) => records,
                Err(problem) => {
                    let aside = path.with_extension("ron.corrupt");
                    error!(
                        file = %path.display(), aside = %aside.display(), %problem,
                        "the ratings file cannot be read: moving it aside and starting a new ladder"
                    );
                    std::fs::rename(&path, &aside)
                        .with_context(|| format!("moving {} aside", path.display()))?;
                    Vec::new()
                }
            },
            Err(problem) if problem.kind() == ErrorKind::NotFound => Vec::new(),
            Err(problem) => {
                return Err(problem).with_context(|| format!("reading {}", path.display()));
            }
        };
        let records: HashMap<PlayerId, Record> = records
            .into_iter()
            .filter_map(|record| match player_id(&record.player) {
                Some(id) => Some((id, record)),
                None => {
                    warn!(player = %record.player, "skipping a ratings entry with a bad key");
                    None
                }
            })
            .collect();
        info!(file = %path.display(), players = records.len(), "ratings loaded");
        Ok(Self {
            records,
            file: Some(spawn_writer(path)),
        })
    }

    /// The player's rating and games played, as a new player if they have not played.
    pub fn standing(&self, id: PlayerId) -> (i32, u32) {
        self.records
            .get(&id)
            .map_or((STARTING_RATING, 0), |record| (record.rating, record.games))
    }

    pub fn profile(&self, id: PlayerId) -> RankedProfile {
        match self.records.get(&id) {
            Some(record) => RankedProfile {
                rating: record.rating,
                games: record.games,
                wins: record.wins,
                place: Some(self.place_of(record)),
            },
            None => RankedProfile {
                rating: STARTING_RATING,
                games: 0,
                wins: 0,
                place: None,
            },
        }
    }

    fn place_of(&self, record: &Record) -> u32 {
        let ahead = self
            .records
            .values()
            .filter(|other| ladder_key(other) < ladder_key(record))
            .count();
        ahead as u32 + 1
    }

    pub fn leaderboard(&self) -> Vec<LeaderboardEntry> {
        self.sorted()
            .into_iter()
            .take(LEADERBOARD_SIZE)
            .enumerate()
            .map(|(index, record)| LeaderboardEntry {
                place: index as u32 + 1,
                nickname: record.nickname.clone(),
                rating: record.rating,
                games: record.games,
            })
            .collect()
    }

    fn sorted(&self) -> Vec<&Record> {
        let mut records: Vec<&Record> = self.records.values().collect();
        records.sort_by_key(|record| ladder_key(*record));
        records
    }

    /// Records a finished match.
    pub fn apply(&mut self, placed: &[Placed]) {
        for player in placed {
            let record = self.records.entry(player.id).or_insert_with(|| Record {
                player: hex(&player.id.0),
                nickname: player.nickname.clone(),
                rating: STARTING_RATING,
                games: 0,
                wins: 0,
            });
            record.nickname.clone_from(&player.nickname);
            record.rating = (record.rating + player.change).max(0);
            record.games += 1;
            record.wins += u32::from(player.won);
            info!(
                player = %player.id, nickname = %player.nickname, change = player.change,
                rating = record.rating, "rating changed"
            );
        }
        self.save();
    }

    fn save(&self) {
        let Some(file) = &self.file else {
            return;
        };
        let records: Vec<Record> = self.sorted().into_iter().cloned().collect();
        let pretty = ron::ser::PrettyConfig::default();
        match ron::ser::to_string_pretty(&records, pretty) {
            Ok(text) => {
                // Fails only if the writer is gone, which is the server stopping.
                let _ = file.send(text);
            }
            Err(problem) => error!(%problem, "the ratings could not be encoded"),
        }
    }
}

/// Best first; between equals, whoever has played more, then by name, so the order is stable.
fn ladder_key(record: &Record) -> (i32, std::cmp::Reverse<u32>, &str) {
    (
        -record.rating,
        std::cmp::Reverse(record.games),
        &record.nickname,
    )
}

/// Writes the file on a task of its own, so a slow disk never holds up the lobbies. Contents
/// queued behind each other are superseded: only the latest is written.
fn spawn_writer(path: PathBuf) -> mpsc::UnboundedSender<String> {
    let (sender, mut receiver) = mpsc::unbounded_channel::<String>();
    tokio::spawn(async move {
        while let Some(mut text) = receiver.recv().await {
            while let Ok(newer) = receiver.try_recv() {
                text = newer;
            }
            let target = path.clone();
            match tokio::task::spawn_blocking(move || write_whole(&target, &text)).await {
                Ok(Ok(())) => {}
                Ok(Err(problem)) => error!("the ratings could not be saved: {problem:#}"),
                Err(problem) => error!("the ratings could not be saved: {problem}"),
            }
        }
    });
    sender
}

fn write_whole(path: &Path, text: &str) -> Result<()> {
    let temporary = path.with_extension("ron.tmp");
    std::fs::write(&temporary, text).with_context(|| format!("writing {}", temporary.display()))?;
    std::fs::rename(&temporary, path).with_context(|| format!("replacing {}", path.display()))?;
    Ok(())
}

fn hex(key: &PublicKey) -> String {
    key.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn player_id(text: &str) -> Option<PlayerId> {
    if text.len() != 64 || !text.is_ascii() {
        return None;
    }
    let mut key = PublicKey::default();
    for (index, byte) in key.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&text[index * 2..index * 2 + 2], 16).ok()?;
    }
    Some(PlayerId(key))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(number: u8) -> PlayerId {
        PlayerId([number; 32])
    }

    fn placed(number: u8, change: i32, won: bool) -> Placed {
        Placed {
            id: id(number),
            nickname: format!("Player {number}"),
            change,
            won,
        }
    }

    #[test]
    fn a_duel_between_equals_moves_half_of_k() {
        assert_eq!(rating_changes(&[(1000, 10), (1000, 10)]), [12, -12]);
        assert_eq!(rating_changes(&[(1000, 0), (1000, 0)]), [24, -24]);
    }

    #[test]
    fn beating_the_favourite_pays_more_than_beating_the_outsider() {
        let upset = rating_changes(&[(1000, 10), (1400, 10)]);
        let expected = rating_changes(&[(1400, 10), (1000, 10)]);
        assert!(upset[0] > 18, "{upset:?}");
        assert!(expected[0] < 6, "{expected:?}");
        assert_eq!(upset[0], -upset[1]);
    }

    /// Winning a race of eight equals moves a rating as far as winning a duel, and the last place
    /// as far the other way; the middle of the field barely moves.
    #[test]
    fn a_big_race_moves_a_rating_no_more_than_a_duel() {
        let changes = rating_changes(&[(1000, 10); 8]);
        assert_eq!(changes[0], 12);
        assert_eq!(changes[7], -12);
        assert!(changes[3].abs() <= 2, "{changes:?}");
        assert!(changes.windows(2).all(|pair| pair[0] >= pair[1]));
    }

    #[test]
    fn a_race_alone_moves_nothing() {
        assert_eq!(rating_changes(&[(1000, 3)]), [0]);
    }

    #[test]
    fn a_new_player_stands_at_the_start_with_no_place() {
        let ratings = Ratings::in_memory();
        assert_eq!(ratings.standing(id(1)), (STARTING_RATING, 0));
        let profile = ratings.profile(id(1));
        assert_eq!(profile.rating, STARTING_RATING);
        assert_eq!(profile.place, None);
        assert!(!profile.placed());
        assert!(ratings.leaderboard().is_empty());
    }

    #[test]
    fn a_match_updates_the_ladder() {
        let mut ratings = Ratings::in_memory();
        ratings.apply(&[placed(1, 24, true), placed(2, -24, false)]);
        ratings.apply(&[placed(1, 20, true), placed(3, -20, false)]);

        assert_eq!(ratings.standing(id(1)), (1044, 2));
        let profile = ratings.profile(id(1));
        assert_eq!((profile.wins, profile.place), (2, Some(1)));
        assert_eq!(ratings.profile(id(2)).place, Some(3));

        let board = ratings.leaderboard();
        assert_eq!(board[0].nickname, "Player 1");
        assert_eq!(board[0].place, 1);
        assert_eq!(board.len(), 3);
    }

    #[test]
    fn a_rating_never_goes_below_zero() {
        let mut ratings = Ratings::in_memory();
        ratings.apply(&[placed(1, -5000, false)]);
        assert_eq!(ratings.standing(id(1)).0, 0);
    }

    #[test]
    fn keys_survive_the_file() {
        let key = [0xAB; 32];
        assert_eq!(player_id(&hex(&key)), Some(PlayerId(key)));
        assert_eq!(player_id("too short"), None);
        assert_eq!(player_id(&"zz".repeat(32)), None);
        assert_eq!(player_id(&"é".repeat(32)), None);
    }

    fn scratch_file(name: &str) -> PathBuf {
        let directory =
            std::env::temp_dir().join(format!("space-race-ratings-{name}-{}", std::process::id()));
        std::fs::create_dir_all(&directory).unwrap();
        directory.join("ratings.ron")
    }

    #[tokio::test]
    async fn the_ladder_survives_a_restart() {
        let path = scratch_file("restart");
        let _ = std::fs::remove_file(&path);
        {
            let mut ratings = Ratings::load(path.clone()).unwrap();
            ratings.apply(&[placed(1, 24, true), placed(2, -24, false)]);
        }
        // The writer is a task of its own: give it the time to finish.
        for _ in 0..200 {
            if path.exists() {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
        let ratings = Ratings::load(path.clone()).unwrap();
        assert_eq!(ratings.standing(id(1)), (1024, 1));
        assert_eq!(ratings.standing(id(2)), (976, 1));
        std::fs::remove_file(&path).unwrap();
    }

    #[tokio::test]
    async fn a_file_that_cannot_be_read_is_moved_aside() {
        let path = scratch_file("corrupt");
        std::fs::write(&path, "this is not a ladder").unwrap();
        let ratings = Ratings::load(path.clone()).unwrap();
        assert!(ratings.leaderboard().is_empty());
        assert!(!path.exists());
        let aside = path.with_extension("ron.corrupt");
        assert_eq!(
            std::fs::read_to_string(&aside).unwrap(),
            "this is not a ladder"
        );
        std::fs::remove_file(&aside).unwrap();
    }
}
