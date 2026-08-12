//! Property tests over fixed-point arithmetic and over `step`.
//!
//! `docs/MILESTONES.md` M1 asks for two things here, and they are different
//! claims:
//!
//! 1. **Fixed-point operations neither panic nor overflow across the legal
//!    input domain.** Stated as: for in-domain operands the `checked_*` form
//!    returns `Some` and agrees with the saturating form. "Does not overflow"
//!    has to be checked against the checked form, because the saturating form
//!    cannot report that it saturated — which is the whole reason the legal
//!    domain is written down rather than inferred.
//! 2. **No input can drive the simulation out of that domain.** The generators
//!    below deliberately produce hostile inputs — coordinates across the entire
//!    `i32` range, handles that name nothing, seats that do not exist, zero
//!    directions — because `docs/SCOPE.md` starts from the axiom that the
//!    client is lying. The assertions are on the *state*, not on the input.
//!
//! An integration test, not a unit test module: it links `sim` the way every
//! other crate will, so anything it can reach is public API and anything it
//! cannot reach is genuinely closed.

// The crate under test denies these at its root. Repeated here because
// crate-level attributes in `lib.rs` do not reach an integration test, and a
// float sneaking into the test that certifies the absence of floats would be a
// particularly poor way to lose the property.
#![deny(clippy::float_arithmetic, unsafe_code)]

use proptest::prelude::*;
use proptest::test_runner::FileFailurePersistence;
use sim::{
    Action, EntityId, Fx, FxVec2, Input, Liveness, MAX_PROJECTILES, Outcome, PLAYER_COUNT,
    PlayerId, RULES, Rng, State, Tick, new_state, step,
};

/// Where a counter-example is written down, and why it is written down at all.
///
/// A property test that finds a bug once and forgets it is a property test that
/// finds the same bug again in six months, or — worse — does not, because the
/// generator happened to sample elsewhere. `proptest-regressions/properties.txt`
/// is committed to the repository, so a case discovered on one machine, or on
/// one platform of the determinism matrix, becomes a case every later run
/// replays first. That is what turns a lucky sample into a permanent test, and
/// it matters most for the target that is hardest to reproduce locally: an
/// aarch64 failure that vanished on the next run would be the single most
/// expensive thing this suite could lose.
///
/// The path is stated rather than left to proptest's default, and that is not
/// tidiness. The default, `SourceParallel`, locates the crate by walking up from
/// the test source looking for `lib.rs` or `main.rs`; an integration test lives
/// in `tests/`, where it finds neither, so it prints a warning into the middle
/// of a failure report and falls back to
/// `sim/tests/properties.proptest-regressions` — a file beside the source,
/// under a name nobody greps for, in a directory nobody thinks to commit. The
/// seeds were being written; they were being written somewhere they would never
/// be checked in from. `Direct` is relative to the package root, which is where
/// `cargo` runs a test binary from, and it puts them in the conventional place.
const REGRESSIONS: &str = "proptest-regressions/properties.txt";

/// The configuration every block below runs under.
///
/// `..ProptestConfig::default()` is load bearing twice: it carries the case
/// count from the `PROPTEST_CASES` environment variable, which is how the CI
/// job raises the budget above the development default of 256, and it is what
/// keeps this from silently pinning any other default proptest gains later.
fn config() -> ProptestConfig {
    ProptestConfig {
        failure_persistence: Some(Box::new(FileFailurePersistence::Direct(REGRESSIONS))),
        ..ProptestConfig::default()
    }
}

/// Half-extent of the legal coordinate domain, in raw Q15.16 units.
///
/// Everything the rules store as a position lives inside this. It is the domain
/// `docs/ARCHITECTURE.md` states, and the bound that makes multiplication
/// closed: `128 * 128 = 16384`, comfortably inside the type's `32767`.
const DOMAIN_RAW: i32 = 128 * 65536;

/// Worst-case departure from unit length after normalising, in raw units.
///
/// Derived rather than tuned. The integer square root understates the true
/// length by at most one raw unit, which inflates the quotient by a factor of
/// at most `1 + 1/len`; at the shortest length the rules will normalise —
/// `RULES.min_direction_length`, raw 4096 — that is 16 raw units. Truncating
/// each component then removes up to `sqrt(2)`. Sixteen and two, doubled for
/// headroom.
const NORMALIZE_TOLERANCE_RAW: i32 = 36;

fn fx_in_domain() -> impl Strategy<Value = Fx> {
    (-DOMAIN_RAW..=DOMAIN_RAW).prop_map(Fx::from_raw)
}

fn vec_in_domain() -> impl Strategy<Value = FxVec2> {
    (fx_in_domain(), fx_in_domain()).prop_map(|(x, y)| FxVec2::new(x, y))
}

/// Any scalar at all, including the two ends of the type. Used where the claim
/// is about totality rather than about accuracy.
fn any_fx() -> impl Strategy<Value = Fx> {
    proptest::num::i32::ANY.prop_map(Fx::from_raw)
}

/// An input a hostile client could send: any coordinate, any handle, any seat.
fn hostile_input(tick: u32) -> impl Strategy<Value = Input> {
    let action = prop_oneof![
        Just(Action::Idle),
        (any_fx(), any_fx()).prop_map(|(x, y)| Action::Move(FxVec2::new(x, y))),
        (any_fx(), any_fx()).prop_map(|(x, y)| Action::Skillshot(FxVec2::new(x, y))),
        proptest::num::u16::ANY.prop_map(|id| Action::Targeted(EntityId(id))),
        proptest::num::u16::ANY.prop_map(|id| Action::Attack(EntityId(id))),
    ];
    (proptest::num::u8::ANY, proptest::num::u32::ANY, action).prop_map(
        move |(player, seq, action)| Input {
            tick: Tick(tick),
            seq,
            player: PlayerId(player),
            action,
        },
    )
}

/// Every invariant the state is supposed to hold, checked after every tick.
fn assert_state_is_legal(state: &State) {
    let extent = RULES.map_half_extent;
    for champion in state.champions() {
        assert!(
            champion.position.x.abs() <= extent && champion.position.y.abs() <= extent,
            "champion left the map at {:?}",
            champion.position
        );
        if let Liveness::Alive { hp } = champion.liveness {
            assert!(hp >= Fx::ZERO, "negative hit points: {hp:?}");
            assert!(
                hp <= RULES.champion_max_hp,
                "hit points above maximum: {hp:?}"
            );
        }
    }
    for tower in state.towers() {
        assert!(tower.hp >= Fx::ZERO, "negative tower hit points");
        assert!(tower.hp <= RULES.tower_max_hp, "tower above maximum");
    }
    assert!(state.projectiles().count() <= MAX_PROJECTILES);
    for projectile in state.projectiles().iter() {
        // A projectile is culled the tick it leaves the map, so it may sit one
        // tick's travel outside it, never more.
        let slack = extent.add(RULES.skillshot_speed);
        assert!(
            projectile.position.x.abs() <= slack && projectile.position.y.abs() <= slack,
            "projectile far outside the map at {:?}",
            projectile.position
        );
        assert!(projectile.remaining <= RULES.skillshot_lifetime_ticks);
    }
}

proptest! {
    #![proptest_config(config())]

    /// Addition, subtraction and multiplication are closed on the coordinate
    /// domain: the checked form always succeeds, so the saturating form never
    /// silently changed a value.
    #[test]
    fn arithmetic_does_not_overflow_in_domain(a in fx_in_domain(), b in fx_in_domain()) {
        prop_assert_eq!(a.checked_add(b), Some(a.add(b)));
        prop_assert_eq!(a.checked_sub(b), Some(a.sub(b)));
        prop_assert_eq!(a.checked_mul(b), Some(a.mul(b)));
    }

    /// Division is in-domain when the quotient is, which for the rules means
    /// dividing a vector component by that vector's own length. Stated as
    /// `|a| <= |b|`, the condition under which the quotient cannot exceed one.
    #[test]
    fn division_does_not_overflow_when_the_quotient_is_representable(
        a in fx_in_domain(),
        b in fx_in_domain(),
    ) {
        prop_assume!(b != Fx::ZERO && a.abs() <= b.abs());
        prop_assert_eq!(a.checked_div(b), Some(a.div(b)));
        prop_assert!(a.div(b).abs() <= Fx::ONE);
    }

    /// Nothing panics anywhere in the type, including at both ends of it. This
    /// is the claim that matters for a server: `step` has no failure path, so
    /// no arithmetic underneath it may have one either.
    #[test]
    fn arithmetic_is_total_over_the_whole_type(a in any_fx(), b in any_fx()) {
        let _ = a.add(b);
        let _ = a.sub(b);
        let _ = a.mul(b);
        let _ = a.div(b);
        let _ = a.neg();
        let _ = a.abs();
        let _ = a.sqrt();
        prop_assert!(a.abs() >= Fx::ZERO);
    }

    /// Division by zero is defined, not trapped.
    #[test]
    fn division_by_zero_saturates_by_sign(a in any_fx()) {
        prop_assert_eq!(a.checked_div(Fx::ZERO), None);
        let expected = if a > Fx::ZERO {
            Fx::MAX
        } else if a < Fx::ZERO {
            Fx::MIN
        } else {
            Fx::ZERO
        };
        prop_assert_eq!(a.div(Fx::ZERO), expected);
    }

    /// Rounding is symmetric about the origin. Truncation toward zero is what
    /// buys this; an arithmetic shift would not.
    #[test]
    fn multiplication_is_symmetric_about_zero(a in fx_in_domain(), b in fx_in_domain()) {
        prop_assert_eq!(a.neg().mul(b), a.mul(b).neg());
        prop_assert_eq!(a.mul(b), b.mul(a));
    }

    #[test]
    fn addition_is_commutative_and_has_an_inverse(a in fx_in_domain(), b in fx_in_domain()) {
        prop_assert_eq!(a.add(b), b.add(a));
        prop_assert_eq!(a.sub(a), Fx::ZERO);
        prop_assert_eq!(a.add(b).sub(b), a);
    }

    /// `sqrt` returns the greatest scalar whose square does not exceed the
    /// input. Checked over `i64` so the assertion does not inherit the rounding
    /// of the thing it is testing.
    #[test]
    fn square_root_is_the_truncated_root(a in 0i32..=i32::MAX) {
        let root = i64::from(Fx::from_raw(a).sqrt().to_raw());
        let scaled = i64::from(a) * 65536;
        prop_assert!(root * root <= scaled, "root too large");
        prop_assert!((root + 1) * (root + 1) > scaled, "root too small");
    }

    #[test]
    fn square_root_of_a_negative_is_zero(a in i32::MIN..0) {
        prop_assert_eq!(Fx::from_raw(a).sqrt(), Fx::ZERO);
    }

    /// Same claim for vector length, and the reason `length_squared_wide`
    /// exists: a squared distance leaves the scalar range long before the
    /// distance does.
    #[test]
    fn length_is_the_truncated_norm(v in vec_in_domain()) {
        let length = i64::from(v.length().to_raw());
        let squared = v.length_squared_wide();
        prop_assert!(length * length <= squared);
        prop_assert!((length + 1) * (length + 1) > squared);
    }

    /// The range predicate must agree with the length it stands in for,
    /// everywhere except within one raw unit of the boundary, where it is the
    /// exact one and `length` is the rounded one.
    #[test]
    fn range_checks_agree_with_distance(a in vec_in_domain(), b in vec_in_domain(), r in fx_in_domain()) {
        prop_assume!(r >= Fx::ZERO);
        let distance = a.distance(b);
        if distance.add(Fx::EPSILON) < r {
            prop_assert!(a.within_range(b, r), "clearly inside");
        }
        if distance > r.add(Fx::EPSILON) {
            prop_assert!(!a.within_range(b, r), "clearly outside");
        }
    }

    /// Normalisation lands within a derived tolerance of unit length, for every
    /// vector the rules will actually normalise.
    #[test]
    fn normalisation_reaches_unit_length(v in vec_in_domain()) {
        prop_assume!(v.length() >= RULES.min_direction_length);
        let length = v.normalize_or_zero().length();
        let error = length.sub(Fx::ONE).abs();
        prop_assert!(
            error <= Fx::from_raw(NORMALIZE_TOLERANCE_RAW),
            "normalised length {length:?} is off by {error:?}"
        );
    }

    /// The same claim where the bound is actually tight.
    ///
    /// Sampling the whole coordinate domain almost never produces a vector
    /// near the minimum length, so the test above passes with a tolerance far
    /// below the derived one and proves less than it appears to. This one
    /// samples only just above the minimum, which is where the integer square
    /// root has the fewest significant bits and the error is largest.
    #[test]
    fn normalisation_is_worst_at_the_shortest_legal_direction(
        x in -8192i32..=8192,
        y in -8192i32..=8192,
    ) {
        let v = FxVec2::new(Fx::from_raw(x), Fx::from_raw(y));
        prop_assume!(v.length() >= RULES.min_direction_length);
        let error = v.normalize_or_zero().length().sub(Fx::ONE).abs();
        prop_assert!(
            error <= Fx::from_raw(NORMALIZE_TOLERANCE_RAW),
            "normalising {v:?} is off by {error:?}"
        );
    }

    #[test]
    fn normalising_a_zero_vector_gives_zero(_ in 0u8..1) {
        prop_assert_eq!(FxVec2::ZERO.normalize_or_zero(), FxVec2::ZERO);
    }

    /// Walking toward a point converges on it and then stays, rather than
    /// oscillating across it — which would write a different position into the
    /// digest on every tick forever.
    #[test]
    fn walking_converges_on_its_destination(
        from in vec_in_domain(),
        to in vec_in_domain(),
    ) {
        let mut position = from;
        for _ in 0..4096 {
            position = position.step_toward(to, RULES.champion_speed);
        }
        prop_assert_eq!(position, to);
        prop_assert_eq!(position.step_toward(to, RULES.champion_speed), to);
    }

    /// The generator is a pure function of its state, which is what makes the
    /// seed the whole of a match's randomness.
    #[test]
    fn the_generator_is_reproducible(seed: u64) {
        let mut first = Rng::from_seed(seed);
        let mut second = Rng::from_seed(seed);
        for _ in 0..64 {
            prop_assert_eq!(first.next_u64(), second.next_u64());
        }
        prop_assert_eq!(first.state(), second.state());
    }

    #[test]
    fn the_generator_respects_its_bound(seed: u64, bound in 1u32..=u32::MAX) {
        let mut rng = Rng::from_seed(seed);
        for _ in 0..64 {
            prop_assert!(rng.below(bound) < bound);
        }
        prop_assert_eq!(Rng::from_seed(seed).below(0), 0);
    }

    /// The headline property of the whole crate, over hostile input: sixty
    /// ticks of nonsense from every seat, and the state is still legal, the
    /// tick still advanced by exactly one each time, and nothing panicked.
    #[test]
    fn hostile_input_cannot_break_the_state(
        seed: u64,
        script in prop::collection::vec(prop::collection::vec(hostile_input(0), 0..4), 60),
    ) {
        let mut state = new_state(seed);
        for (tick, inputs) in script.iter().enumerate() {
            let retimed: Vec<Input> = inputs
                .iter()
                .map(|input| Input { tick: state.tick(), ..*input })
                .collect();
            let next = step(&state, &retimed);
            prop_assert_eq!(next.tick(), Tick(tick as u32 + 1));
            assert_state_is_legal(&next);
            state = next;
        }
    }

    /// `step` is a function: same arguments, same result, and its argument is
    /// left alone. Cheap to state, and it is the assumption every replay
    /// verification makes.
    #[test]
    fn step_is_pure(seed: u64, inputs in prop::collection::vec(hostile_input(0), 0..8)) {
        let state = new_state(seed);
        let before = state.digest();
        let first = step(&state, &inputs);
        let second = step(&state, &inputs);
        prop_assert_eq!(first.digest(), second.digest());
        prop_assert_eq!(state.digest(), before);
    }

    /// Two states that differ anywhere must digest differently, and two that
    /// agree everywhere must digest identically. The second half is what the
    /// determinism suite relies on; the first is what makes it meaningful.
    #[test]
    fn the_digest_follows_the_state(seed_a: u64, seed_b: u64) {
        prop_assume!(seed_a != seed_b);
        let a = new_state(seed_a);
        let b = new_state(seed_b);
        prop_assert_ne!(a.digest(), b.digest());
        prop_assert_eq!(a.digest(), new_state(seed_a).digest());
    }

    /// A decided match stays decided, whatever anyone sends afterwards.
    #[test]
    fn an_outcome_is_never_revised(seed: u64) {
        let mut state = new_state(seed);
        let mut decided_at = None;
        for _ in 0..200 {
            state = step(&state, &[]);
            match (state.outcome(), decided_at) {
                (Outcome::Decided { winner, at }, None) => decided_at = Some((winner, at)),
                (Outcome::Decided { winner, at }, Some(previous)) => {
                    prop_assert_eq!((winner, at), previous);
                }
                (Outcome::InProgress, Some(_)) => prop_assert!(false, "outcome un-decided itself"),
                (Outcome::InProgress, None) => {}
            }
        }
    }
}

/// Every seat can be driven, and the seat that acts is the seat named. Not a
/// property test — six cases is the whole domain.
#[test]
fn each_seat_responds_to_its_own_input_only() {
    let state = new_state(11);
    for seat in 0..PLAYER_COUNT {
        let destination = FxVec2::new(Fx::ZERO, Fx::ZERO);
        let next = step(
            &state,
            &[Input {
                tick: Tick(0),
                seq: 0,
                player: PlayerId(seat as u8),
                action: Action::Move(destination),
            }],
        );
        for other in 0..PLAYER_COUNT {
            let moved = next.champions()[other].position != state.champions()[other].position;
            assert_eq!(
                moved,
                other == seat,
                "seat {other} moved on seat {seat}'s input"
            );
        }
    }
}

/// A seat number outside the match is ignored rather than folded back into the
/// match by a modulo — the obvious wrong fix, and one that would let a client
/// drive somebody else's champion.
#[test]
fn a_seat_that_does_not_exist_is_ignored() {
    let state = new_state(11);
    for player in [6u8, 7, 100, u8::MAX] {
        let next = step(
            &state,
            &[Input {
                tick: Tick(0),
                seq: 0,
                player: PlayerId(player),
                action: Action::Move(FxVec2::ZERO),
            }],
        );
        assert_eq!(next.champions(), state.champions(), "seat {player}");
    }
}
