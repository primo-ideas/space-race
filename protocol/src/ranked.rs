//! The ranked ladder, as both sides need to name it: the tiers a rating falls in and how many games
//! a new player's rating counts as provisional. How ratings move is the server's alone
//! (`server/src/ranking.rs`).

use bitcode::{Decode, Encode};

/// Games a new player plays before they are placed in a tier. Their rating moves faster meanwhile.
pub const PLACEMENT_GAMES: u32 = 5;

/// A band of ratings.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Encode, Decode)]
pub enum Tier {
    Bronze,
    Silver,
    Gold,
    Platinum,
    Diamond,
    Champion,
}

impl Tier {
    /// The tier a rating falls in. A new player starts at 1000, in Silver.
    pub fn of(rating: i32) -> Self {
        match rating {
            ..950 => Self::Bronze,
            950..1100 => Self::Silver,
            1100..1250 => Self::Gold,
            1250..1400 => Self::Platinum,
            1400..1600 => Self::Diamond,
            _ => Self::Champion,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Bronze => "BRONZE",
            Self::Silver => "SILVER",
            Self::Gold => "GOLD",
            Self::Platinum => "PLATINUM",
            Self::Diamond => "DIAMOND",
            Self::Champion => "CHAMPION",
        }
    }
}

/// Where a player stands on the ladder.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Encode, Decode)]
pub struct RankedProfile {
    pub rating: i32,
    pub games: u32,
    pub wins: u32,
    /// Their place among everyone who has played, from 1. `None` before their first game.
    pub place: Option<u32>,
}

impl RankedProfile {
    /// Whether the player's placement games are over and their tier means something.
    pub fn placed(&self) -> bool {
        self.games >= PLACEMENT_GAMES
    }
}

/// A line of the leaderboard.
#[derive(Debug, Clone, PartialEq, Eq, Encode, Decode)]
pub struct LeaderboardEntry {
    pub place: u32,
    pub nickname: String,
    pub rating: i32,
    pub games: u32,
}

/// How the ranked queue stands, for a player in it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Encode, Decode)]
pub struct QueueStatus {
    /// Everyone in the queue, this player included.
    pub players: u16,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tiers_follow_the_rating() {
        assert_eq!(Tier::of(-5), Tier::Bronze);
        assert_eq!(Tier::of(949), Tier::Bronze);
        assert_eq!(Tier::of(950), Tier::Silver);
        assert_eq!(Tier::of(1000), Tier::Silver);
        assert_eq!(Tier::of(1100), Tier::Gold);
        assert_eq!(Tier::of(1250), Tier::Platinum);
        assert_eq!(Tier::of(1400), Tier::Diamond);
        assert_eq!(Tier::of(1599), Tier::Diamond);
        assert_eq!(Tier::of(1600), Tier::Champion);
        assert_eq!(Tier::of(5000), Tier::Champion);
    }
}
