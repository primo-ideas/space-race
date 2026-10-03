//! A small random generator (SplitMix64) for what the client draws without its being written down
//! anywhere: the towers of a district, the stars of the sky. The same seed always rolls the same
//! numbers, on every client, so the city and the sky are the same for everyone.

pub struct Dice(u64);

impl Dice {
    pub fn new(seed: u64) -> Self {
        Self(seed)
    }

    pub fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut mixed = self.0;
        mixed = (mixed ^ (mixed >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        mixed = (mixed ^ (mixed >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        mixed ^ (mixed >> 31)
    }

    /// A number from `low` up to, but not including, `high`.
    pub fn range(&mut self, low: f32, high: f32) -> f32 {
        let unit = (self.next() >> 40) as f32 / (1u64 << 24) as f32;
        low + (high - low) * unit
    }

    /// A whole number from zero up to, but not including, `count`.
    pub fn below(&mut self, count: usize) -> usize {
        (self.next() % count as u64) as usize
    }
}

/// A seed made of a few numbers, such as a prop's own, so what is drawn from them always comes out
/// the same.
pub fn seed(numbers: &[f32]) -> u64 {
    numbers.iter().fold(0x5EED_CA5E, |seed, number| {
        Dice::new(seed ^ u64::from(number.to_bits())).next()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_same_seed_rolls_the_same_numbers_within_range() {
        let (mut first, mut second) = (Dice::new(7), Dice::new(7));
        for _ in 0..1000 {
            let roll = first.range(2.0, 5.0);
            assert_eq!(roll, second.range(2.0, 5.0));
            assert!((2.0..5.0).contains(&roll), "{roll}");
            assert!(first.below(3) < 3);
            second.below(3);
        }
        assert_ne!(seed(&[1.0, 2.0]), seed(&[2.0, 1.0]));
    }
}
