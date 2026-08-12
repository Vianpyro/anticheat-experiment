//! Framing, and the constant size that is the point of it.
//!
//! A frame is a version, a kind, a payload, and zeroes to the end. Both frame
//! types are newtypes over fixed-size arrays, which is what makes "every frame
//! of a given direction is the same length" a property of the type rather than
//! of a test somebody has to remember to write.

use sim::view::PlayerView;
use sim::{Digest, Seat};

use crate::message::{ClientMessage, RejectReason, ServerMessage};
use crate::wire::{Reader, Writer};

/// Version and kind, before every payload.
pub const HEADER_BYTES: usize = 3;

/// Every client frame is exactly this long.
///
/// The widest payload is an `Input`: a sequence number (4), a claimed timestamp
/// (8), and the widest action, which is a tag plus a point (9).
///
/// The client's own traffic is padded too, and the reason is *not* the culling
/// invariant — nothing a client sends depends on what it can see, so there is
/// no maphack channel here to close. It is padded because the cost is nine
/// bytes a frame and the alternative is a length that classifies the action: an
/// observer who can tell a `Move` from an `Idle` by counting bytes is reading
/// play they were not given. That is a smaller claim than the server-side one
/// and it is made here rather than left implicit.
pub const CLIENT_FRAME_BYTES: usize = HEADER_BYTES + 4 + 8 + 9;

/// Every server frame is exactly this long.
///
/// The bucket is the header plus [`PlayerView::MAX_ENCODED_BYTES`], which is
/// derived from the view encoding rather than measured from a run. One bucket,
/// sized for the worst case: see the crate documentation for why several
/// buckets is not a cheaper version of the same property, and
/// `docs/ARCHITECTURE.md` for what the padding costs in bandwidth.
///
/// `Accepted` and `Rejected` are padded to the same width as a `View`. They
/// are sent once, at the start of a session, where a distinguishable length
/// would say nothing an observer could not infer from the fact that a session
/// just started — so this buys little. It costs one constant instead of two,
/// and it means there is no second frame size in the system to reason about
/// later.
pub const SERVER_FRAME_BYTES: usize = HEADER_BYTES + PlayerView::MAX_ENCODED_BYTES;

/// Why a frame did not decode.
///
/// Every one of these is a protocol violation by the sender. The server's
/// answer to all of them is the same — refuse the session — so the distinction
/// is for the operator reading a log and for the tests, not for control flow.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DecodeError {
    /// The buffer was not exactly one frame long.
    Length {
        /// What a frame of this direction is.
        expected: usize,
        /// What arrived.
        actual: usize,
    },
    /// The frame names a protocol this build does not speak.
    Version {
        /// What this build speaks.
        expected: u16,
        /// What the frame claimed.
        found: u16,
    },
    /// The kind byte names no message.
    Kind(u8),
    /// The payload did not parse: a tag naming no variant, or a field that ran
    /// off the end of the frame.
    Body,
    /// A byte after the payload was not zero.
    ///
    /// Refused rather than ignored, and the reason is not tidiness. Padding a
    /// receiver skips is a channel a sender can write to, and one message must
    /// have exactly one encoding for the recorded input log to be a function of
    /// what was played. A frame that decodes two ways is a frame two verifiers
    /// can disagree about.
    Padding,
    /// A seat byte that names no seat.
    ///
    /// This is the frontier `sim::Seat` exists for: the byte becomes a seat
    /// here or the frame is refused here, and nothing downstream has a case to
    /// handle.
    Seat(u8),
}

impl core::fmt::Display for DecodeError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Length { expected, actual } => {
                write!(f, "frame is {actual} bytes, expected exactly {expected}")
            }
            Self::Version { expected, found } => {
                write!(
                    f,
                    "frame claims protocol {found}, this build speaks {expected}"
                )
            }
            Self::Kind(tag) => write!(f, "kind byte {tag} names no message"),
            Self::Body => write!(f, "payload did not parse"),
            Self::Padding => write!(f, "a byte after the payload was not zero"),
            Self::Seat(byte) => write!(f, "seat byte {byte} names no seat"),
        }
    }
}

impl core::error::Error for DecodeError {}

const KIND_JOIN: u8 = 0;
const KIND_READY: u8 = 1;
const KIND_INPUT: u8 = 2;
const KIND_SURRENDER: u8 = 3;

const KIND_ACCEPTED: u8 = 0;
const KIND_REJECTED: u8 = 1;
const KIND_VIEW: u8 = 2;

/// One client frame: exactly [`CLIENT_FRAME_BYTES`] bytes.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct ClientFrame([u8; CLIENT_FRAME_BYTES]);

/// One server frame: exactly [`SERVER_FRAME_BYTES`] bytes.
///
/// Not `Copy`, and not `Debug`-printed in full: it is one and a half kilobytes,
/// almost all of it padding.
#[derive(Clone, PartialEq, Eq)]
pub struct ServerFrame([u8; SERVER_FRAME_BYTES]);

impl core::fmt::Debug for ClientFrame {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "ClientFrame({CLIENT_FRAME_BYTES} bytes)")
    }
}

impl core::fmt::Debug for ServerFrame {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "ServerFrame({SERVER_FRAME_BYTES} bytes)")
    }
}

/// Pads a payload out to a frame, or reports that it did not fit.
///
/// Not fitting is a bug in this crate rather than a runtime condition — the
/// frame constants are derived from the widest payload — so the `Option` exists
/// to keep the function total rather than to describe something that happens.
/// The tests below encode the widest message of each direction and require the
/// result to be exactly one frame, which is what keeps the derivation honest.
fn pad<const N: usize>(kind: u8, payload: &[u8]) -> Option<[u8; N]> {
    let mut frame = [0u8; N];
    let version = crate::VERSION.to_be_bytes();
    *frame.get_mut(0)? = version[0];
    *frame.get_mut(1)? = version[1];
    *frame.get_mut(2)? = kind;
    let body = frame.get_mut(HEADER_BYTES..HEADER_BYTES.checked_add(payload.len())?)?;
    body.copy_from_slice(payload);
    Some(frame)
}

/// Splits a frame into its kind byte and its payload, after checking the
/// version. The payload includes the padding; the caller checks it.
fn open(bytes: &[u8], expected_len: usize) -> Result<(u8, &[u8]), DecodeError> {
    if bytes.len() != expected_len {
        return Err(DecodeError::Length {
            expected: expected_len,
            actual: bytes.len(),
        });
    }
    let version = u16::from_be_bytes([bytes[0], bytes[1]]);
    if version != crate::VERSION {
        return Err(DecodeError::Version {
            expected: crate::VERSION,
            found: version,
        });
    }
    Ok((bytes[2], &bytes[HEADER_BYTES..]))
}

/// Requires everything the reader did not consume to be zero.
fn check_padding(payload: &[u8], consumed: usize) -> Result<(), DecodeError> {
    match payload.get(consumed..) {
        Some(rest) if rest.iter().all(|byte| *byte == 0) => Ok(()),
        _ => Err(DecodeError::Padding),
    }
}

impl ClientFrame {
    /// The frame's bytes.
    #[must_use]
    pub const fn as_bytes(&self) -> &[u8; CLIENT_FRAME_BYTES] {
        &self.0
    }

    /// Wraps bytes that arrived. No validation: [`ClientFrame::decode`] is
    /// where a frame becomes a message.
    #[must_use]
    pub const fn from_bytes(bytes: [u8; CLIENT_FRAME_BYTES]) -> Self {
        Self(bytes)
    }

    /// Encodes a message into a frame.
    #[must_use]
    pub fn encode(message: &ClientMessage) -> Self {
        let mut writer = Writer::with_capacity(CLIENT_FRAME_BYTES);
        let kind = match *message {
            ClientMessage::Join => KIND_JOIN,
            ClientMessage::Ready => KIND_READY,
            ClientMessage::Input {
                seq,
                claimed_at_ms,
                action,
            } => {
                writer.u32(seq);
                writer.u64(claimed_at_ms);
                writer.action(action);
                KIND_INPUT
            }
            ClientMessage::Surrender => KIND_SURRENDER,
        };
        let payload = writer.finish();
        Self(pad(kind, &payload).expect("a client payload wider than a client frame"))
    }

    /// Reads a frame that arrived from a client.
    ///
    /// # Errors
    ///
    /// Every way a byte string can fail to be exactly one well-formed frame,
    /// including a payload followed by padding that is not zero.
    pub fn decode(bytes: &[u8]) -> Result<ClientMessage, DecodeError> {
        let (kind, payload) = open(bytes, CLIENT_FRAME_BYTES)?;
        let mut reader = Reader::new(payload);
        let message = match kind {
            KIND_JOIN => ClientMessage::Join,
            KIND_READY => ClientMessage::Ready,
            KIND_INPUT => {
                let seq = reader.u32().ok_or(DecodeError::Body)?;
                let claimed_at_ms = reader.u64().ok_or(DecodeError::Body)?;
                let action = reader.action().ok_or(DecodeError::Body)?;
                ClientMessage::Input {
                    seq,
                    claimed_at_ms,
                    action,
                }
            }
            KIND_SURRENDER => ClientMessage::Surrender,
            other => return Err(DecodeError::Kind(other)),
        };
        check_padding(payload, reader.consumed())?;
        Ok(message)
    }
}

impl ServerFrame {
    /// The frame's bytes.
    #[must_use]
    pub const fn as_bytes(&self) -> &[u8; SERVER_FRAME_BYTES] {
        &self.0
    }

    /// Wraps bytes that arrived.
    #[must_use]
    pub const fn from_bytes(bytes: [u8; SERVER_FRAME_BYTES]) -> Self {
        Self(bytes)
    }

    /// Encodes a message into a frame.
    ///
    /// The return type is the invariant: there is no way for this function to
    /// produce a frame whose length depends on the message it was given.
    #[must_use]
    pub fn encode(message: &ServerMessage) -> Self {
        let mut writer = Writer::with_capacity(SERVER_FRAME_BYTES);
        let kind = match message {
            ServerMessage::Accepted {
                seat,
                seed,
                rules_hash,
            } => {
                writer.u8(seat.index() as u8);
                writer.u64(*seed);
                writer.bytes(rules_hash.as_bytes());
                KIND_ACCEPTED
            }
            ServerMessage::Rejected(reason) => {
                writer.u8(reason.tag());
                KIND_REJECTED
            }
            ServerMessage::View(view) => {
                writer.bytes(&view.encode());
                KIND_VIEW
            }
        };
        let payload = writer.finish();
        Self(pad(kind, &payload).expect("a server payload wider than a server frame"))
    }

    /// Reads a frame that arrived from the server.
    ///
    /// # Errors
    ///
    /// As [`ClientFrame::decode`], plus a seat byte that names no seat.
    pub fn decode(bytes: &[u8]) -> Result<ServerMessage, DecodeError> {
        let (kind, payload) = open(bytes, SERVER_FRAME_BYTES)?;
        let mut reader = Reader::new(payload);
        let message = match kind {
            KIND_ACCEPTED => {
                let byte = reader.u8().ok_or(DecodeError::Body)?;
                let seat = Seat::from_index(byte).ok_or(DecodeError::Seat(byte))?;
                let seed = reader.u64().ok_or(DecodeError::Body)?;
                let rules_hash = Digest::from_bytes(reader.array::<32>().ok_or(DecodeError::Body)?);
                ServerMessage::Accepted {
                    seat,
                    seed,
                    rules_hash,
                }
            }
            KIND_REJECTED => {
                let tag = reader.u8().ok_or(DecodeError::Body)?;
                ServerMessage::Rejected(RejectReason::from_tag(tag).ok_or(DecodeError::Body)?)
            }
            KIND_VIEW => {
                let (view, read) = PlayerView::decode(payload).ok_or(DecodeError::Body)?;
                check_padding(payload, read)?;
                return Ok(ServerMessage::View(view));
            }
            other => return Err(DecodeError::Kind(other)),
        };
        check_padding(payload, reader.consumed())?;
        Ok(message)
    }
}
