//! The train/holdout split, frozen before the first detector exists.
//!
//! `docs/MILESTONES.md` M6 asks for "a frozen train/holdout split", and
//! `docs/RISKS.md` R8 says why the freezing is the whole of it: a split chosen
//! after a threshold has been looked at is not a holdout, it is a second training
//! set with a reassuring name. So it is fixed here, in a commit that lands before
//! M8, and the two constants below are what "frozen" means.
//!
//! # It is a function and not a file
//!
//! The obvious implementation is a list of match identifiers in a file. It is
//! also, exactly, the derived index M5 removed from this corpus and
//! `docs/CONSENT.md` records the lesson from: a second place a match is named, an
//! artefact that can outlive what it describes, and one more thing a withdrawal
//! has to reach. A withdrawal that destroyed a match and left it named in a split
//! file would leave behind a line saying that a match that no longer exists was
//! held out — which is a fact about somebody's participation, written down after
//! they asked for it to be destroyed.
//!
//! So the assignment is a **pure function of the match identifier**, computed
//! when somebody asks and stored nowhere. Three things follow, and the third is
//! the one that made the decision:
//!
//! - There is nothing to withdraw. The audit has no new file to find.
//! - There is nothing to keep in step. A corpus copied to another machine splits
//!   the same way because the rule travels in the code.
//! - **It is stable under withdrawal.** A rule like "the first four fifths by
//!   date" reassigns every match the moment one is destroyed, so a participant
//!   exercising their right to withdraw would silently move matches from the
//!   holdout into the training set — and a threshold already chosen would have
//!   been chosen on data that is now training data. A hash of the identifier does
//!   not move.
//!
//! # What the rule is, and what it is not
//!
//! One match in [`HOLDOUT_IN`] is held out, chosen by the digest of
//! [`SALT`] and the match identifier. It is a hash and not a random draw: the
//! same corpus splits the same way on every machine and in every year, which is
//! what makes a published statistic reproducible.
//!
//! It is **not stratified**, and that is a limitation rather than an oversight. A
//! nine-person corpus of a few dozen matches has no room for stratification that
//! would mean anything: holding out by *person* would put a fifth of the
//! participants entirely outside training, which is the split a
//! person-generalisation claim would need and which nine people cannot afford;
//! holding out by *match* is what this does, and the claim it supports is
//! correspondingly narrower. `docs/SCHEMA.md` states the consequence beside the
//! two bounds, because a reader shown a holdout and not shown what it holds out
//! has been handled.

use sim::digest_bytes;

use crate::manifest::MatchId;

/// The string mixed into the identifier before hashing.
///
/// Frozen. Changing it reshuffles every match in the corpus, which is the same
/// act as choosing a new split after looking at the data — so it may only change
/// alongside a decision to discard every result computed under the old one, and
/// `docs/SCHEMA.md` says so.
pub const SALT: &[u8] = b"moba/holdout/v1";

/// One match in this many is held out.
///
/// Four, so that a corpus of the size M6 can realistically reach — twenty to
/// forty matches — puts five to ten matches beyond the reach of every threshold.
/// A fifth or a tenth would be tidier fractions and would leave a holdout of two
/// or four matches, which is a holdout that cannot distinguish a detector from a
/// coin.
pub const HOLDOUT_IN: u32 = 4;

/// Which half of the corpus a match belongs to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Split {
    /// A detector may look at it.
    Train,
    /// A detector may not, until its threshold is fixed and published.
    Holdout,
}

impl Split {
    /// The tag this split is written as.
    #[must_use]
    pub const fn tag(self) -> &'static str {
        match self {
            Self::Train => "train",
            Self::Holdout => "holdout",
        }
    }
}

/// Which half this match belongs to.
///
/// A total function of the identifier and of two constants. It reads no file,
/// consults no corpus, and gives the same answer on every machine.
#[must_use]
pub fn split_of(match_id: MatchId) -> Split {
    let mut bytes = Vec::with_capacity(SALT.len().saturating_add(16));
    bytes.extend_from_slice(SALT);
    bytes.extend_from_slice(&match_id.0);
    let digest = digest_bytes(&bytes);
    let first: [u8; 4] = digest
        .as_bytes()
        .get(..4)
        .and_then(|slice| slice.try_into().ok())
        .unwrap_or([0; 4]);
    if u32::from_be_bytes(first).is_multiple_of(HOLDOUT_IN) {
        Split::Holdout
    } else {
        Split::Train
    }
}

#[cfg(test)]
mod tests {
    use super::{HOLDOUT_IN, Split, split_of};
    use crate::manifest::MatchId;

    fn identifier(index: u64) -> MatchId {
        let mut bytes = [0u8; 16];
        bytes[..8].copy_from_slice(&index.to_be_bytes());
        bytes[8..].copy_from_slice(&index.wrapping_mul(0x9E37_79B9_7F4A_7C15).to_be_bytes());
        MatchId(bytes)
    }

    /// The same identifier always lands in the same half.
    #[test]
    fn the_split_is_a_function_of_the_identifier_alone() {
        for index in 0..64u64 {
            assert_eq!(split_of(identifier(index)), split_of(identifier(index)));
        }
    }

    /// **Destroying a match does not move any other match.**
    ///
    /// The property a list in a file does not have, and the reason this is a
    /// function. A participant withdrawing must not reshuffle a split a
    /// threshold was already chosen against.
    #[test]
    fn a_withdrawal_cannot_move_a_match_from_one_half_to_the_other() {
        let corpus: Vec<MatchId> = (0..40u64).map(identifier).collect();
        let before: Vec<Split> = corpus.iter().map(|id| split_of(*id)).collect();

        // Every match a hypothetical participant played in, gone.
        let survivors: Vec<MatchId> = corpus
            .iter()
            .enumerate()
            .filter(|(index, _)| !index.is_multiple_of(3))
            .map(|(_, id)| *id)
            .collect();
        assert!(survivors.len() < corpus.len(), "nothing was destroyed");

        for identifier in &survivors {
            let position = corpus
                .iter()
                .position(|other| other == identifier)
                .expect("a survivor was in the corpus");
            assert_eq!(
                split_of(*identifier),
                before[position],
                "a match changed halves when another match was destroyed"
            );
        }
    }

    /// The rule reaches both halves, and roughly at the rate it claims.
    ///
    /// `docs/RISKS.md` R15: a split that assigned everything to one half would
    /// satisfy every property above and would be no split at all. The bounds are
    /// wide because 256 draws of a one-in-four Bernoulli is a wide thing; what
    /// they refuse is a constant.
    #[test]
    fn the_rule_reaches_both_halves_at_about_the_declared_rate() {
        let held = (0..256u64)
            .map(identifier)
            .filter(|id| split_of(*id) == Split::Holdout)
            .count();
        println!("split: {held} of 256 held out, one in {HOLDOUT_IN} declared");
        assert!(
            held > 40 && held < 88,
            "{held} of 256 held out, against an expected 64"
        );
    }
}
