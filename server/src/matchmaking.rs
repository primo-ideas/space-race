//! Who plays whom in ranked: the rule that picks a match out of the queue, apart from the queue
//! itself so it can be tested on its own.
//!
//! A player waits for opponents close to their rating, and the longer they have waited the further
//! they look, until after about a minute they will play anyone: with few players online, an uneven
//! race is better than none, and the ratings allow for it. A full match starts at once; a smaller
//! one once its oldest player has waited long enough for others to have turned up.

use std::time::Duration;

/// Players in a full match.
pub const MATCH_SIZE: usize = 4;
/// Fewest players a match can have.
const SMALLEST_MATCH: usize = 2;

/// How far from their rating a player who has just arrived looks, in points, and how much further
/// for every second they wait.
const WINDOW: f32 = 150.0;
const WINDOW_PER_SECOND: f32 = 15.0;

/// Someone in the queue.
#[derive(Debug, Clone, Copy)]
pub struct Candidate {
    pub rating: i32,
    pub waited: Duration,
}

impl Candidate {
    fn window(&self) -> f32 {
        WINDOW + WINDOW_PER_SECOND * self.waited.as_secs_f32()
    }

    /// Either of the two has looked far enough to accept the other.
    fn accepts(&self, other: &Self) -> bool {
        (self.rating - other.rating).abs() as f32 <= self.window().max(other.window())
    }
}

/// The next match to start, as indices into `queue`, which is listed longest waiting first: the
/// players closest to the one who has waited longest. A match smaller than full starts only once
/// that player has waited `gather`.
pub fn pick(queue: &[Candidate], gather: Duration) -> Option<Vec<usize>> {
    queue.iter().enumerate().find_map(|(anchor, candidate)| {
        let mut group: Vec<usize> = (0..queue.len())
            .filter(|&other| other == anchor || candidate.accepts(&queue[other]))
            .collect();
        group.sort_by_key(|&other| {
            (
                other != anchor,
                (queue[other].rating - candidate.rating).abs(),
            )
        });
        group.truncate(MATCH_SIZE);
        let ready = group.len() == MATCH_SIZE
            || (group.len() >= SMALLEST_MATCH && candidate.waited >= gather);
        ready.then_some(group)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const GATHER: Duration = Duration::from_secs(15);

    fn waiting(rating: i32, seconds: u64) -> Candidate {
        Candidate {
            rating,
            waited: Duration::from_secs(seconds),
        }
    }

    #[test]
    fn nobody_plays_alone() {
        assert_eq!(pick(&[], GATHER), None);
        assert_eq!(pick(&[waiting(1000, 600)], GATHER), None);
    }

    #[test]
    fn two_players_give_others_time_to_turn_up() {
        assert_eq!(pick(&[waiting(1000, 3), waiting(1010, 0)], GATHER), None);
        assert_eq!(
            pick(&[waiting(1000, 15), waiting(1010, 0)], GATHER),
            Some(vec![0, 1])
        );
    }

    #[test]
    fn a_full_match_starts_at_once() {
        let queue = [
            waiting(1000, 1),
            waiting(1020, 1),
            waiting(980, 0),
            waiting(1050, 0),
        ];
        assert_eq!(pick(&queue, GATHER).map(|group| group.len()), Some(4));
    }

    #[test]
    fn a_match_takes_the_closest_ratings() {
        let queue = [
            waiting(1000, 30),
            waiting(1300, 1),
            waiting(1010, 1),
            waiting(990, 1),
            waiting(1005, 1),
            waiting(1200, 1),
        ];
        let group = pick(&queue, GATHER).unwrap();
        assert_eq!(group[0], 0, "the one who waited longest is in it");
        assert_eq!(group.len(), MATCH_SIZE);
        for far in [1, 5] {
            assert!(!group.contains(&far), "{group:?}");
        }
    }

    /// Far apart, two players do not meet at first, and do once they have waited long enough for
    /// one of them to reach the other.
    #[test]
    fn the_longer_a_player_waits_the_further_they_look() {
        assert_eq!(pick(&[waiting(1000, 20), waiting(1800, 20)], GATHER), None);
        assert!(pick(&[waiting(1000, 44), waiting(1800, 0)], GATHER).is_some());
    }

    /// Whoever is closest to the first player is only one candidate: if they cannot play yet, the
    /// next pair in the queue still can.
    #[test]
    fn a_pair_further_down_the_queue_can_start() {
        let queue = [waiting(2000, 20), waiting(1000, 16), waiting(1010, 16)];
        assert_eq!(pick(&queue, GATHER), Some(vec![1, 2]));
    }
}
