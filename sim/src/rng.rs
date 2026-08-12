//! The seeded pseudo-random generator carried inside [`crate::State`].
//!
//! `rand` is not a dependency and will not become one: `docs/RISKS.md` R9 is
//! about nondeterminism smuggled in by a crate rather than written by hand, and
//! a generator whose algorithm can change in a patch release is exactly that
//! shape. SplitMix64 is reproduced here in full — it is nine lines, its
//! constants are published, and it cannot drift.
//!
//! It is not a cryptographic generator and must not be used as one. Nothing in
//! the rules needs unpredictability; it needs *reproducibility*, which is the
//! opposite property.
//!
//! # Current consumers: none
//!
//! No rule in the frozen MVP draws from it — there are no critical strikes, no
//! random spread, no jungle spawns. It exists because the seed is part of the
//! state's identity from the first commit rather than retrofitted into a replay
//! format later, and because the alternative to threading it now is threading
//! it through `step` the day one rule wants it. Ties are broken by `EntityId`
//! and never by this generator: an ordering that depends on the draw order is
//! an ordering that changes when an unrelated rule starts drawing.

/// SplitMix64. Reproducible across platforms because every operation is
/// wrapping integer arithmetic on `u64`, which has no implementation freedom.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rng {
    pub(crate) state: u64,
}

/// The golden-ratio increment from the reference implementation.
const GAMMA: u64 = 0x9E37_79B9_7F4A_7C15;
const MIX_A: u64 = 0xBF58_476D_1CE4_E5B9;
const MIX_B: u64 = 0x94D0_49BB_1331_11EB;

impl Rng {
    /// Seeds the generator. Every seed is valid, including zero.
    #[must_use]
    pub const fn from_seed(seed: u64) -> Self {
        Self { state: seed }
    }

    /// The current internal state. This is the whole of the generator's
    /// identity, which is what makes it hashable into the state digest.
    #[must_use]
    pub const fn state(self) -> u64 {
        self.state
    }

    /// Advances the generator and returns the next value.
    pub const fn next_u64(&mut self) -> u64 {
        self.state = self.state.wrapping_add(GAMMA);
        let mut z = self.state;
        z = (z ^ (z >> 30)).wrapping_mul(MIX_A);
        z = (z ^ (z >> 27)).wrapping_mul(MIX_B);
        z ^ (z >> 31)
    }

    /// Advances the generator and returns the high 32 bits, which are the
    /// better-mixed half.
    pub const fn next_u32(&mut self) -> u32 {
        (self.next_u64() >> 32) as u32
    }

    /// A value in `[0, bound)`, or `0` when `bound` is zero.
    ///
    /// Uses the multiply-and-shift reduction rather than a modulo. Both are
    /// biased; this one is biased by at most `bound / 2^32` and costs no
    /// branch, whereas a rejection loop would consume a variable number of
    /// draws and make the generator's position depend on the values it drew.
    /// For a simulation that has to resimulate identically, a fixed number of
    /// draws per call is worth more than the last bit of uniformity.
    pub const fn below(&mut self, bound: u32) -> u32 {
        if bound == 0 {
            return 0;
        }
        ((self.next_u32() as u64).saturating_mul(bound as u64) >> 32) as u32
    }
}
