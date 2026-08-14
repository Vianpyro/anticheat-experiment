//! What a team is *entitled* to see, re-derived from the rules.
//!
//! # Why this is not read off the projection, which is what it used to be
//!
//! Every exploit about vision compares what an attacker learned against what the
//! attacker was allowed to learn, and the second half cannot come from the
//! function under test. It did, in the first draft of `tests/maphack.rs`, and the
//! mutation exercise found it: with culling deliberately removed, the exploit did
//! not go red at the exploit — it went red at its own `docs/RISKS.md` R15
//! antecedent, because the broken projection had redefined "hidden". The test was
//! asserting that `view_for` agrees with itself, which a projection that leaks
//! everything satisfies as long as it leaks consistently.
//!
//! `docs/ARCHITECTURE.md` invariant 5 refuses that shape for `sim`'s own
//! visibility suite and `sim/tests/spec/mod.rs` is the re-derivation it uses
//! instead. This is the same move, one crate over, and it carries the same
//! obligation: **a change to the vision rule changes this file in the same
//! commit.** The duplication is the mechanism, not an accident of it.

use sim::{Liveness, Seat, State, TOWER_COUNT, Team, tower_position};

/// Whether `team` is entitled to see `point`: the union of its living champions'
/// and its standing towers' vision discs.
///
/// The specification `sim::view::can_see` implements, written out again.
/// `docs/MILESTONES.md` M2 records the one time the two diverged — truncated
/// distances against exact squares, a shell one raw unit thick outside every
/// circle — which is the argument for having a second statement of it rather than
/// the argument against.
#[must_use]
pub fn team_can_see(state: &State, team: Team, point: sim::FxVec2) -> bool {
    for seat in Seat::ALL {
        if seat.team() != team {
            continue;
        }
        let champion = state.champion(seat);
        if matches!(champion.liveness, Liveness::Alive { .. })
            && champion
                .position
                .within_range(point, sim::RULES.champion_vision_radius)
        {
            return true;
        }
    }
    for index in 0..TOWER_COUNT {
        if sim::tower_team(index) == team
            && state.towers()[index].is_standing()
            && tower_position(index, &sim::RULES)
                .within_range(point, sim::RULES.tower_vision_radius)
        {
            return true;
        }
    }
    false
}
