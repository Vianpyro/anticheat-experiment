//! The wire: message types, framing, versioning, and the per-recipient handle
//! space.
//!
//! This crate is the trust boundary. `docs/SCOPE.md` starts from the axiom that
//! the client is compromised and lying and that the protocol is the only surface
//! an attacker legitimately has, so everything a client can send is decoded
//! here, in one place, by code that is total on every byte string.
//!
//! # The traffic-shape invariant
//!
//! `docs/ARCHITECTURE.md` states it in two halves and both of them are load
//! bearing: **one message per player per tick, at a constant cadence, of a
//! constant encoded size, independent of content.** Culling is worth nothing
//! without it — an observer who cannot read a single byte still counts packets
//! and measures their length, and either number is close to a linear readout of
//! how many entities a player can see.
//!
//! This crate owns the size half and makes it structural rather than
//! asserted. [`ServerFrame`] is a newtype over `[u8; SERVER_FRAME_BYTES]`, so
//! "every server frame is the same size" is a fact about the type: there is no
//! encoder that could return a shorter one and no test that could catch a
//! bucketing scheme, because a bucketing scheme would not compile. What the
//! property tests in `server` carry instead is the half a type cannot state —
//! that the *bytes* a player receives are a function of what that player is
//! entitled to know, and that the number of frames is a function of the tick
//! count and nothing else.
//!
//! The cadence half is a statement about a loop and lives in `server`, because
//! no encoding can make it.
//!
//! # One bucket, and it is the worst case
//!
//! `SERVER_FRAME_BYTES` is the frame header plus
//! [`sim::view::PlayerView::MAX_ENCODED_BYTES`], which is derived from the
//! encoding rather than measured from a run. There is exactly one bucket, and
//! that is the whole design: a scheme with several buckets is only
//! content-independent if the bucket is chosen without looking at the content,
//! and a bucket chosen without looking at the content is a single bucket with
//! extra steps. Anything smaller would have to drop entities to fit, and which
//! entities get dropped is content. The bandwidth this costs is arithmetic
//! rather than a guess, and `docs/ARCHITECTURE.md` carries the calculation
//! under "The padding budget".
//!
//! # Handles a recipient is given rather than told
//!
//! Champion and tower handles are public: a champion's handle *is* its seat and
//! a tower's position follows from the rules. Projectile handles are not. They
//! come from a counter that is global to the match, so a client that sees `1005`
//! and then `1009` has learned that three skillshots were cast where it could
//! not see them. [`HandleSpace`] closes that by giving every recipient a handle
//! space of its own; see its documentation for the shape that was chosen and
//! the one that was rejected.

#![forbid(unsafe_code)]
#![deny(missing_docs, missing_debug_implementations)]

mod frame;
mod handles;
mod message;
mod wire;

pub use crate::frame::{
    CLIENT_FRAME_BYTES, ClientFrame, DecodeError, HEADER_BYTES, SERVER_FRAME_BYTES, ServerFrame,
};
pub use crate::handles::HandleSpace;
pub use crate::message::{ClientMessage, RejectReason, ServerMessage};

/// The protocol this build speaks.
///
/// Carried in the header of **every** frame rather than exchanged once during a
/// handshake. That costs two bytes in a frame that is padded anyway, and it buys
/// a property worth more than the bytes: there is no window in which a session
/// has been accepted and the two ends still disagree about how to read the next
/// message. A frame whose version is not this one is refused without being
/// parsed, which is also the only safe order to do it in.
///
/// It is a wire-format number and not the workspace's version. It moves when
/// the meaning of a byte changes, and `sim`'s own version — which moves when
/// the *rules* change, `docs/RISKS.md` R13 — is a separate claim about a
/// separate thing.
pub const VERSION: u16 = 1;
