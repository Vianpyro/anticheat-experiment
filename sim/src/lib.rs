//! The rules of the game, and the verification kernel everything else rests on.
//!
//! `sim` is a pure function from a state and a slice of inputs to the next
//! state. It has no clock, no I/O, no async, no threads, no floating point and
//! no dependencies — not "few", none: its `[dependencies]` table is empty and
//! is meant to stay that way (`docs/RISKS.md` R9). The same `step` runs in the
//! server, in replay verification, in the determinism suite, and eventually in
//! the reinforcement-learning environment, and any upward dependency would let
//! those four consumers drift apart.
//!
//! # The property everything else is built on
//!
//! The same seed and the same input log produce the same [`State::digest`] on
//! x86-64 Linux, x86-64 Windows and aarch64 macOS. Replays, resimulation, the
//! detector calibration and the anti-cheat premise itself all rest on that one
//! sentence. `docs/RISKS.md` R1 explains why it is the first risk in the
//! document and why it is enforced by lints rather than by review.
//!
//! # The legal domain
//!
//! Determinism is not the same as "cannot go wrong", and this crate states
//! where it is defined rather than hoping the question does not come up. The
//! full statement, with the reasoning, is in `docs/ARCHITECTURE.md`; the short
//! version is:
//!
//! - Coordinates are in `[-128, 128]` on both axes. Anything a client sends
//!   outside that is clamped in, never rejected.
//! - A per-tick displacement is at most 16 units.
//! - Inside that domain no [`Fx`] operation saturates, which the property tests
//!   assert directly by requiring the `checked_*` form to return `Some`.
//! - Outside it, arithmetic **saturates**. It does not wrap and it does not
//!   panic. See [`Fx`] for why those two were rejected.
//!
//! # What this crate deliberately does not have
//!
//! No `Champion` trait, no ability system, no entity-component framework, no
//! event bus. There is one champion and five actions, and
//! `docs/ARCHITECTURE.md` sets the bar an abstraction has to clear: more than
//! one implementation, iterated over by a caller. Nothing here clears it.
//!
//! And no serialization. [`State`] implements no encoding, in this crate or
//! anywhere in the workspace, which is what turns "the server must not send the
//! whole world to a client" from a code-review habit into a compile error
//! (`docs/RISKS.md` R5).

#![forbid(unsafe_code)]
// The lints are the enforcement mechanism, not the discipline. Each of these
// corresponds to a documented risk, and lifting one locally requires an
// `#[expect]` or `#[allow]` carrying a reason.
#![deny(
    // R1: floats cannot be made bit-identical across x86-64 and aarch64.
    clippy::float_arithmetic,
    // Overflow semantics must be chosen at every site rather than inherited
    // from the build profile. Every arithmetic operation in this crate names
    // what it does on overflow: `saturating_*`, `checked_*` or `wrapping_*`.
    clippy::arithmetic_side_effects,
    // R1, R9: the `clippy.toml` beside this file lists what may not be named.
    clippy::disallowed_types,
    // Load bearing for `canonical`: a field bound during exhaustive
    // destructuring and then not hashed must not compile. Denied here rather
    // than relied upon from CI's `-D warnings`, so the invariant holds under a
    // plain `cargo build` too.
    unused_variables,
    missing_docs,
    missing_debug_implementations
)]

mod canonical;
mod event;
mod fx;
mod input;
mod rng;
mod rules;
mod sha256;
mod state;
mod step;
mod vec2;

/// The visibility projection, and the only types in this crate that are meant
/// to leave the server.
///
/// A module of its own rather than a handful of re-exports at the root, because
/// the boundary is the point: every call site reads `sim::view::…`, and a grep
/// for that path is a grep for everything that can reach a client
/// (`docs/RISKS.md` R5).
pub mod view;

pub use crate::event::{Ability, Event, EventKind, Events, MAX_EVENTS};
pub use crate::fx::{FRAC_BITS, Fx};
pub use crate::input::{Action, Input};
pub use crate::rng::Rng;
pub use crate::rules::{RULES, Rules, TICKS_PER_SECOND, rules_hash};
pub use crate::sha256::Digest;
pub use crate::state::{
    Champion, Cooldowns, EntityId, EntityRef, Liveness, MAX_PROJECTILES, Order, Outcome,
    PLAYER_COUNT, PROJECTILE_ID_BASE, Projectile, Projectiles, Seat, State, TEAM_COUNT, TEAM_SIZE,
    TOWER_COUNT, TOWER_ID_BASE, Team, Tick, Tower, base_position, champion_entity_id, entity_id,
    entity_team, new_state, new_state_with_rules, tower_entity_id, tower_lane, tower_position,
    tower_team,
};
pub use crate::step::{step, step_with_rules};
pub use crate::vec2::FxVec2;

/// The SHA-256 of a byte string, under this crate's own implementation.
///
/// Exposed for one reason and it is worth naming, because a hash function on a
/// public API invites uses this one is not for. The headless client at M3 has
/// to digest the world it reconciled from the views it was sent, and the whole
/// point of that digest is that three clients and the server compute the *same*
/// 32 bytes over related material. A second SHA-256 in another crate would be a
/// second implementation to keep agreeing with this one, and a dependency on
/// `sha2` in `client` would be a dependency that can change under a project
/// whose central claim is byte-identical results.
///
/// It is not a commitment scheme, a MAC, or a signature primitive. M5's
/// signatures take an audited crate: a security portfolio does not hand-roll
/// signature code, and hand-writing the hash beneath one is the same mistake
/// with an extra step.
#[must_use]
pub fn digest_bytes(bytes: &[u8]) -> Digest {
    let mut hasher = crate::sha256::Hasher::new();
    hasher.update(bytes);
    hasher.finish()
}

/// The digest of an input log, in the order given.
///
/// Belongs here rather than in `replay` because the ordering rule it encodes is
/// a `sim` rule: `step` does not sort and does not deduplicate, so two logs
/// that differ only in order are two different logs. The replay manifest will
/// carry this value at M5 (`docs/RISKS.md` R4), and it must be the same
/// function that produced the log the server resimulated.
#[must_use]
pub fn input_log_digest(inputs: &[Input]) -> Digest {
    crate::canonical::digest_of_inputs(inputs)
}
