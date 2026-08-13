//! Framing, and the constant size that is the point of it.
//!
//! A frame is a version, a kind, a payload, and zeroes to the end. Both frame
//! types are newtypes over fixed-size arrays, which is what makes "every frame
//! of a given direction is the same length" a property of the type rather than
//! of a test somebody has to remember to write.
//!
//! # A server frame is cut into a constant number of constant-size shards
//!
//! A `View` frame does not travel as one write. It is cut into
//! [`SERVER_SHARDS`] datagrams of exactly [`SERVER_DATAGRAM_BYTES`] bytes each,
//! and that geometry is the whole padding scheme: *constant cadence, constant
//! count, constant size*. An observer counts the same number of packets of the
//! same length every tick whatever the match is doing, which is the property a
//! single padded frame on a reliable stream also had — and this version has it
//! without the reliable stream, so a lost packet costs one tick's frame instead
//! of blocking every frame behind it.
//!
//! The shard is the unit the network sees, so the shard is where the size
//! constraint has to hold: [`SERVER_DATAGRAM_BYTES`] is asserted against
//! [`MAX_DATAGRAM_BYTES`] at compile time. `docs/ARCHITECTURE.md` carries the
//! arithmetic under "The padding budget".

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

/// Datagrams one server frame is cut into.
///
/// Two, and the number is chosen rather than derived: it is the smallest count
/// that puts a shard comfortably under every path MTU this project will meet,
/// given a frame whose entity list alone is most of a kilobyte. One shard would
/// work on a 1200-byte path and fail on a tunnelled one, and the failure mode of
/// a datagram that does not fit is a frame that is never sent — a cadence gap,
/// which is the half of the traffic invariant padding cannot repair.
pub const SERVER_SHARDS: usize = 2;

/// The largest datagram this protocol will put on the wire.
///
/// A budget rather than a measurement of any particular path. QUIC's own floor
/// is a 1200-byte UDP payload and real paths are usually 1500 minus headers;
/// 600 leaves room for a tunnel, an IPv6 header and a QUIC packet header
/// several times over. It is asserted against, not aimed at: the constant below
/// stops the build if the frame outgrows it, which is the day somebody has to
/// choose between a third shard and a smaller view.
pub const MAX_DATAGRAM_BYTES: usize = 600;

/// A shard's own header: the protocol version, the frame it belongs to, and
/// which shard of that frame it is.
///
/// The version is repeated here rather than read out of the frame's own header,
/// which only shard zero carries. A shard from another protocol is then refused
/// before it is put anywhere, which is the same "check before you parse" order
/// the frame header already imposes; the cost is two bytes in a frame that is
/// padded anyway.
///
/// The frame number is a per-session counter of frames sent, which is a count
/// of ticks: public by construction, since the cadence is one frame per player
/// per tick whatever happened.
pub const SHARD_HEADER_BYTES: usize = 2 + 4 + 1;

/// The input acknowledgement that precedes a view: a tag byte and four bytes of
/// sequence number, present in both cases.
///
/// Constant width rather than "one byte when there is nothing to acknowledge",
/// for the reason `Outcome` is padded in `sim`'s own encoding: a variant that
/// costs fewer bytes is a variant an observer can read off a length. Here it
/// would say "this session has not had an input applied yet", which is a fact
/// about the start of a match and worth exactly as little as it costs to hide.
pub const APPLIED_BYTES: usize = 1 + 4;

/// What a frame has to hold: the header, the acknowledgement, and the widest
/// view the encoding can produce.
const FRAME_MINIMUM: usize = HEADER_BYTES + APPLIED_BYTES + PlayerView::MAX_ENCODED_BYTES;

/// Bytes of frame in one shard. The frame is rounded up to a whole number of
/// these, so the padding a receiver checks covers the rounding too.
pub const SERVER_SHARD_PAYLOAD_BYTES: usize = FRAME_MINIMUM.div_ceil(SERVER_SHARDS);

/// Every server frame is exactly this long.
///
/// The bucket is the header plus [`PlayerView::MAX_ENCODED_BYTES`], which is
/// derived from the view encoding rather than measured from a run, rounded up
/// to a whole number of shards. One bucket, sized for the worst case: see the
/// crate documentation for why several buckets is not a cheaper version of the
/// same property, and `docs/ARCHITECTURE.md` for what the padding costs in
/// bandwidth.
///
/// `Accepted` and `Rejected` are padded to the same width as a `View`. They
/// are sent once, at the start of a session, where a distinguishable length
/// would say nothing an observer could not infer from the fact that a session
/// just started — so this buys little. It costs one constant instead of two,
/// and it means there is no second frame size in the system to reason about
/// later.
pub const SERVER_FRAME_BYTES: usize = SERVER_SHARDS * SERVER_SHARD_PAYLOAD_BYTES;

/// Every datagram carrying part of a server frame is exactly this long.
pub const SERVER_DATAGRAM_BYTES: usize = SHARD_HEADER_BYTES + SERVER_SHARD_PAYLOAD_BYTES;

/// The constraint the whole scheme exists to satisfy. A frame that outgrows the
/// shard geometry stops the build here rather than becoming a datagram the
/// transport silently refuses to send.
const _: () = assert!(SERVER_DATAGRAM_BYTES <= MAX_DATAGRAM_BYTES);

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
    /// A shard that claims to be part of a frame it could not be part of.
    ///
    /// There are exactly [`SERVER_SHARDS`] shards in a frame, numbered from
    /// zero, and a datagram naming any other index is refused rather than
    /// clamped: a receiver that folded a bad index into a real one would let a
    /// sender overwrite a shard it did not send.
    ShardIndex(u8),
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
            Self::ShardIndex(index) => {
                write!(
                    f,
                    "shard index {index}, of a frame that has {SERVER_SHARDS}"
                )
            }
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
            ServerMessage::View {
                view,
                applied_through,
            } => {
                // The acknowledgement first, at a constant width, so that the
                // view's own variable-length encoding starts at a fixed offset
                // and the decoder does not have to find it.
                match applied_through {
                    Some(seq) => {
                        writer.u8(1);
                        writer.u32(*seq);
                    }
                    None => {
                        writer.u8(0);
                        writer.u32(0);
                    }
                }
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
                let tag = reader.u8().ok_or(DecodeError::Body)?;
                let seq = reader.u32().ok_or(DecodeError::Body)?;
                let applied_through = match tag {
                    // The absent case carries four bytes of nothing, and they
                    // are required to be nothing: a sender that wrote a sequence
                    // number behind a `None` tag would have given one message
                    // two encodings, which is the whole reason padding is
                    // checked rather than skipped.
                    0 if seq == 0 => None,
                    1 => Some(seq),
                    _ => return Err(DecodeError::Body),
                };
                let consumed = reader.consumed();
                let rest = payload.get(consumed..).ok_or(DecodeError::Body)?;
                let (view, read) = PlayerView::decode(rest).ok_or(DecodeError::Body)?;
                check_padding(payload, consumed.saturating_add(read))?;
                return Ok(ServerMessage::View {
                    view,
                    applied_through,
                });
            }
            other => return Err(DecodeError::Kind(other)),
        };
        check_padding(payload, reader.consumed())?;
        Ok(message)
    }

    /// Cuts the frame into the datagrams that carry it.
    ///
    /// `sequence` names the frame within the session and is the same for all
    /// [`SERVER_SHARDS`] shards; it is what lets a receiver tell this frame's
    /// shards from the next one's, and what lets it abandon a frame whose shard
    /// never arrived rather than sew two half-frames together.
    ///
    /// The return type is the invariant, in the same way [`ServerFrame`]'s own
    /// is: there is no way for this function to produce a different number of
    /// datagrams, or datagrams of a different size, for one message than for
    /// another.
    #[must_use]
    pub fn shards(&self, sequence: u32) -> [ServerShard; SERVER_SHARDS] {
        core::array::from_fn(|index| {
            let mut bytes = [0u8; SERVER_DATAGRAM_BYTES];
            bytes[0..2].copy_from_slice(&crate::VERSION.to_be_bytes());
            bytes[2..6].copy_from_slice(&sequence.to_be_bytes());
            // `SERVER_SHARDS` is small and this is its index, so the conversion
            // cannot lose anything; it is written to saturate rather than to
            // panic because a panic in the send path is a match everybody
            // loses.
            bytes[6] = u8::try_from(index).unwrap_or(u8::MAX);
            let from = index * SERVER_SHARD_PAYLOAD_BYTES;
            let to = from + SERVER_SHARD_PAYLOAD_BYTES;
            bytes[SHARD_HEADER_BYTES..].copy_from_slice(&self.0[from..to]);
            ServerShard(bytes)
        })
    }
}

/// One datagram: a shard header and a slice of exactly one frame.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct ServerShard([u8; SERVER_DATAGRAM_BYTES]);

impl core::fmt::Debug for ServerShard {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "ServerShard({SERVER_DATAGRAM_BYTES} bytes)")
    }
}

impl ServerShard {
    /// The datagram's bytes.
    #[must_use]
    pub const fn as_bytes(&self) -> &[u8; SERVER_DATAGRAM_BYTES] {
        &self.0
    }
}

/// Puts the shards of a frame back together, and gives up on the ones that will
/// not be whole.
///
/// # What a missing shard costs, and why that is the trade
///
/// Datagrams are unreliable and unordered, so a frame is delivered only when
/// all [`SERVER_SHARDS`] of its shards arrive. A frame with a shard missing is
/// abandoned the moment a *newer* frame's first shard arrives: the recipient
/// misses one tick and reads the next one, which is what a client that predicts
/// wants. The alternative — waiting for the retransmission that a reliable
/// stream would have made — is head-of-line blocking, and at 30 Hz it stalls
/// every subsequent tick behind one lost packet.
///
/// # Ordering
///
/// Only the newest frame is held. A shard belonging to an older frame is
/// discarded rather than buffered, because a view older than the one the client
/// already applied is a view it would discard anyway
/// (`client::Headless::reconcile`), and a reassembler that kept a window would
/// be a second place where delivery order could turn into observable state.
#[derive(Clone, Debug)]
pub struct ShardAssembler {
    /// The frame being assembled, if any, and which of its shards have arrived.
    building: Option<u32>,
    present: [bool; SERVER_SHARDS],
    /// Whether that frame has already been handed out. Held so that a duplicate
    /// shard is stale rather than the start of a second delivery of one tick.
    delivered: bool,
    buffer: [u8; SERVER_FRAME_BYTES],
    /// Frames abandoned because a shard never arrived.
    incomplete: u32,
    /// Shards discarded for naming a frame that is no longer the newest.
    stale: u32,
}

impl Default for ShardAssembler {
    fn default() -> Self {
        Self::new()
    }
}

impl ShardAssembler {
    /// An assembler that has been given nothing.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            building: None,
            present: [false; SERVER_SHARDS],
            delivered: false,
            buffer: [0u8; SERVER_FRAME_BYTES],
            incomplete: 0,
            stale: 0,
        }
    }

    /// Frames abandoned because one of their shards never arrived.
    #[must_use]
    pub const fn incomplete(&self) -> u32 {
        self.incomplete
    }

    /// Shards discarded for arriving after a newer frame had started.
    #[must_use]
    pub const fn stale(&self) -> u32 {
        self.stale
    }

    /// Takes one datagram, and returns a frame once its last shard arrives.
    ///
    /// # Errors
    ///
    /// [`DecodeError`] for a datagram that is not a shard of this protocol: the
    /// wrong length, another version, or an index no frame has. These come off
    /// a socket, so they are refused rather than assumed away.
    pub fn accept(&mut self, bytes: &[u8]) -> Result<Option<ServerFrame>, DecodeError> {
        if bytes.len() != SERVER_DATAGRAM_BYTES {
            return Err(DecodeError::Length {
                expected: SERVER_DATAGRAM_BYTES,
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
        let sequence = u32::from_be_bytes([bytes[2], bytes[3], bytes[4], bytes[5]]);
        let index = usize::from(bytes[6]);
        if index >= SERVER_SHARDS {
            return Err(DecodeError::ShardIndex(bytes[6]));
        }

        match self.building {
            // A shard of the frame already handed out, or of one superseded by
            // a newer arrival. Either way there is nothing left to add it to.
            Some(current) if current == sequence && self.delivered => {
                self.stale = self.stale.saturating_add(1);
                return Ok(None);
            }
            // A shard of the frame being assembled.
            Some(current) if current == sequence => {}
            Some(current) if sequence < current => {
                self.stale = self.stale.saturating_add(1);
                return Ok(None);
            }
            // A newer frame. Whatever was half-assembled is never completed.
            Some(_) => {
                if !self.delivered {
                    self.incomplete = self.incomplete.saturating_add(1);
                }
                self.start(sequence);
            }
            None => self.start(sequence),
        }

        let from = index * SERVER_SHARD_PAYLOAD_BYTES;
        let to = from + SERVER_SHARD_PAYLOAD_BYTES;
        self.buffer[from..to].copy_from_slice(&bytes[SHARD_HEADER_BYTES..]);
        self.present[index] = true;

        if !self.present.iter().all(|have| *have) {
            return Ok(None);
        }
        self.delivered = true;
        Ok(Some(ServerFrame(self.buffer)))
    }

    fn start(&mut self, sequence: u32) {
        self.building = Some(sequence);
        self.present = [false; SERVER_SHARDS];
        self.delivered = false;
        self.buffer = [0u8; SERVER_FRAME_BYTES];
    }
}
