//! Exploit class 5: protocol abuse — replay, reordering, out-of-sequence,
//! unvalidated handles, and messages that do not belong in a session's state.
//!
//! # Which layer this is about, stated because `docs/SCOPE.md` requires it
//!
//! QUIC already defeats the naive attack: a duplicated or reordered *packet* is
//! rejected beneath the application, and this crate cannot reach that layer to
//! test it because `quinn` does not hand it a duplicated packet to send. What is
//! left — and what these exploits are about — is the **application-level
//! residue**: two *distinct* packets carrying the same intention, an input whose
//! sequence number went backwards, a `Join` from a session that already has a
//! seat, a handle that names no entity. `docs/SCOPE.md`'s note on class 5 asks
//! for exactly this distinction, and `Match::deliver` is where the residue is
//! handled.
//!
//! # These exploits produce frames; the harness sends them
//!
//! Every function here is a frame or a sequence of frames an attacker would
//! send. What each one does to a running match is asserted in `tests/abuse.rs`,
//! against a real in-process server — because "the server rejects Y" is a claim
//! about the server, and the only honest way to make it is to send Y to one.
//!
//! The weakened-defence half of M7 is different in kind for this class and the
//! tests say so at each site: the defences here are not a projection that can be
//! switched off but sequence and session rules in `Match::deliver`. So the "it
//! would have worked" half is established by showing the frame is *well-formed
//! and accepted at the transport* — it decodes, it is a legal `ClientMessage` —
//! and would be acted upon by a server without the rule. An exploit that failed
//! because its frame did not even decode would be `docs/RISKS.md` R15 again: a
//! refusal one step short of the check it is about.

use protocol::{Action, ClientFrame, ClientMessage, EntityId};

/// A single intention frame carrying the sequence number and action given.
///
/// The building block for the sequence attacks: the attacker chooses the number
/// rather than incrementing it, which is the whole of what makes a frame a
/// replay or a reorder.
#[must_use]
pub fn input(seq: u32, action: Action, claimed_at_ms: u64) -> ClientFrame {
    ClientFrame::encode(&ClientMessage::Input {
        seq,
        claimed_at_ms,
        action,
    })
}

/// The same intention frame twice: a replay at the application level.
///
/// Two distinct packets carrying one intention. QUIC cannot tell they are the
/// same because at its layer they are not — they are two datagrams with the same
/// payload, which a client is free to send. The defence is `Match::deliver`'s
/// strictly-increasing sequence rule, and `tests/abuse.rs` asserts that the
/// second is refused and applied zero times.
#[must_use]
pub fn replayed_input(seq: u32, action: Action) -> [ClientFrame; 2] {
    [input(seq, action, 0), input(seq, action, 0)]
}

/// Two intentions in descending sequence order: a reorder.
///
/// The server accepts a sequence number strictly greater than the last it
/// accepted, so once the higher number lands the lower one is a no-op. The
/// attacker sends them high-then-low; the low one is what must be refused.
#[must_use]
pub fn out_of_order(low: u32, high: u32, action: Action) -> [ClientFrame; 2] {
    [input(high, action, 0), input(low, action, 0)]
}

/// A `Join` frame, for the attacker who sends a second one on an established
/// session.
///
/// `Join` is how a connection asks for a seat. A session that already has a seat
/// sending another is a client whose state machine does not match the server's,
/// and `Match::deliver` answers it with `Violation::OutOfOrder` — the exploit is
/// that a naive server would allocate a second seat or overwrite the first.
#[must_use]
pub fn second_join() -> ClientFrame {
    ClientFrame::encode(&ClientMessage::Join)
}

/// An `Attack` order naming a handle that resolves to nothing.
///
/// `docs/ARCHITECTURE.md`'s `apply_inputs` discards an order to attack an ally, a
/// corpse, a rubble heap or an entity that never existed, *leaving the previous
/// order in place*, because storing an arbitrary handle would let an attacker
/// write it into the state. This is the frame that probes it: a well-formed
/// `Input` whose action carries a handle far outside every entity's range.
///
/// The number is chosen to be past champions, past towers, and past any
/// projectile handle a match of this length could allocate.
///
/// Takes a sequence number because it is sent mid-session, after other inputs:
/// a hard-coded `0` would be refused by the sequence rule before the handle rule
/// ever ran, which is a different exploit reaching a different check.
#[must_use]
pub fn attack_nonexistent(seq: u32) -> ClientFrame {
    input(seq, Action::Attack(EntityId(60_000)), 0)
}

/// An `Attack` order naming the attacker's own teammate.
///
/// A handle that resolves — to a champion — but to one on the attacker's own
/// team, which the same rule discards. It is a distinct probe from the
/// unresolvable handle because it exercises the *team* check rather than the
/// resolution check, and a server that dropped only the second would let a
/// player order friendly fire.
#[must_use]
pub fn attack_own_ally(seq: u32, ally: EntityId) -> ClientFrame {
    input(seq, Action::Attack(ally), 0)
}

/// A raw, arbitrary byte string, for the attacker who sends something that is not
/// a frame at all.
///
/// The frontier `docs/SCOPE.md` puts first: everything arriving over the wire is
/// hostile and is decoded in one place. A server that panicked, allocated without
/// bound, or acted on a partial parse would be the vulnerability; the defence is
/// that `ClientFrame::decode` is total and `Match::deliver` turns a decode
/// failure into a `Violation` the session owns.
#[must_use]
pub fn garbage(bytes: Vec<u8>) -> Vec<u8> {
    bytes
}
