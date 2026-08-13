//! The transport, and the only part of this crate that has a clock.
//!
//! # Why QUIC
//!
//! `docs/RISKS.md` R6 decided it at M3 and this is where the decision lands.
//! What it buys, and the honesty R6 demands about which layer is doing the work:
//!
//! - **Transport-level encryption and packet-level anti-replay come from QUIC,
//!   not from this project.** A duplicated or reordered *packet* is rejected
//!   beneath the application. What remains for exploit class 5 is the
//!   application-level residue — session commands that must be idempotent, and
//!   input sequence numbers that must be monotone — and that residue is handled
//!   in `Match::deliver`, not here. `docs/SCOPE.md` requires this distinction to
//!   be stated rather than claimed as a defence.
//! - **No hand-rolled cryptography**, which is the failure mode a security
//!   portfolio cannot afford.
//!
//! # Datagrams for state, one reliable stream for the session
//!
//! This is `docs/RISKS.md` R6's hedge as it was originally written, and it took
//! a smaller frame to get back to it. A `ServerFrame` used to be 1501 bytes,
//! which does not fit a QUIC datagram, so every frame travelled on a reliable
//! stream and a lost packet blocked the frames behind it — head-of-line
//! blocking at 30 Hz, which is the wrong netcode for anything a client
//! predicts from.
//!
//! A frame is now cut into [`protocol::SERVER_SHARDS`] datagrams of exactly
//! [`protocol::SERVER_DATAGRAM_BYTES`] bytes, each far inside any path MTU. A
//! lost packet costs the tick it belonged to and nothing else: the client
//! abandons the half-assembled frame when the next one starts arriving, and
//! reads on.
//!
//! The traffic-shape invariant is not weakened by that, it is stated more
//! directly than before. It used to be "a constant number of bytes at a
//! constant period is packetised into a constant number of packets", which is
//! an argument about QUIC's packetiser; it is now a constant number of
//! datagrams of a constant size at a constant period, which is a fact about
//! this file. `docs/ARCHITECTURE.md` records the arithmetic under the padding
//! budget.
//!
//! **The session keeps its stream, and that split is the design.** `Accepted`
//! and `Rejected` are sent once and must arrive, so they go on the one
//! bidirectional stream; a client's own frames go the same way, because `Ready`
//! and `Surrender` are session commands with the same requirement and an
//! upstream frame is 24 bytes, where head-of-line blocking costs one tick's
//! intention that the sequence rule already treats as droppable. State is the
//! only traffic that is better lost than late, and state is the only traffic
//! that travels unreliably.
//!
//! # The certificate
//!
//! A self-signed certificate generated at startup, whose DER the client is
//! given out of band and trusts exactly. There is no certificate authority and
//! no name resolution because there is one server process and the exploit suite
//! boots it in-process. What this deliberately is **not** is a client that skips
//! verification: a custom `ServerCertVerifier` that accepts anything is forty
//! lines of `rustls` boilerplate whose only purpose is to disable the thing
//! `rustls` is for, and a security portfolio with one of those in it is a
//! security portfolio with a hole in it and a comment beside it.

use std::net::SocketAddr;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use bytes::Bytes;
use protocol::{CLIENT_FRAME_BYTES, ServerFrame};
use quinn::{Connection, Endpoint, RecvStream, SendStream};
use replay::Recording;
use sim::{PLAYER_COUNT, Rng, Seat};
use tokio::sync::mpsc;

use crate::{Match, MatchConfig, Violation};

/// Why the transport gave up.
#[derive(Debug)]
pub enum NetError {
    /// The socket, the endpoint, or a stream.
    Io(std::io::Error),
    /// Generating or installing the certificate.
    Certificate(String),
    /// A connection ended before the match did.
    Connection(String),
}

impl core::fmt::Display for NetError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Io(error) => write!(f, "{error}"),
            Self::Certificate(what) => write!(f, "certificate: {what}"),
            Self::Connection(what) => write!(f, "connection: {what}"),
        }
    }
}

impl core::error::Error for NetError {}

impl From<std::io::Error> for NetError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}

/// The server's own clock, in milliseconds since the epoch.
///
/// The only clock in the system that is evidence of anything (`docs/SCOPE.md`,
/// adversary model): the client's own timestamp is attacker-controlled by
/// definition, and the divergence between the two is exploit class 4's signal.
/// It is read here, at the edge, and passed into the authority as an argument —
/// which is what keeps `Match` a function of its inputs.
fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| {
            u64::try_from(since.as_millis()).unwrap_or(u64::MAX)
        })
}

/// How long the shutdown gives the driver to put the last datagrams on the
/// wire before the endpoint is closed.
///
/// There is nothing to *wait on* for a datagram: it is unacknowledged by
/// construction, so `SendStream::stopped` has no counterpart here and a server
/// that waited for one would wait for a client that is not reading a stream.
/// So the drain is a bounded pause and not a handshake, and a frame that misses
/// it is a frame lost exactly as any other datagram can be — which is the
/// semantics the state channel already has.
const DRAIN: Duration = Duration::from_millis(200);

/// Datagrams this server throws away on purpose.
///
/// # Why a server has a knob for breaking itself
///
/// The state channel is unreliable by construction and the whole of the M3 exit
/// criterion had to be weakened to say so — no two clients ever disagree about a
/// checkpoint they *both received*, plus a floor of nine tenths of the
/// checkpoints reached. On loopback the observed loss is zero, so that floor had
/// never once been approached and its calibration was unknown; `ShardAssembler`
/// was covered by unit tests that hand it shards in an order chosen by hand, and
/// nothing had ever exercised loss end to end, through a real endpoint, against
/// a client that was reconciling at the same time.
///
/// A test that cannot produce the condition it is about is a test about
/// something else. So the loss is injected here, at the one place where dropping
/// a datagram is indistinguishable from a network dropping it: the send path,
/// after the frame has been cut into shards. Each shard is drawn for
/// independently, which is the model that matters — a frame needs both of its
/// shards, so a per-shard rate of *p* is a per-frame delivery rate of
/// `(1 - p)²`, and the exit criterion's floor is a statement about the second
/// number and not the first.
///
/// # Why it is safe to have in the shipping crate
///
/// It defaults to nothing, `moba-server` has no argument that reaches it, and
/// the worst an operator can do with it is degrade their own players' sessions —
/// there is no client-supplied field anywhere near it and nothing an attacker
/// can steer. What it buys is that the loss tests run the *same* transport the
/// game runs, rather than a second one written to be droppable.
///
/// The draw is `sim::Rng` from a written-down seed, not an ambient generator, so
/// a run that goes red can be repeated.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LossProfile {
    /// Percentage of outgoing datagrams to drop, per shard, independently.
    /// Clamped to `0..=100`.
    pub datagram_percent: u8,
    /// The seed the draws come from. Sessions are offset from it by seat, so
    /// two sessions do not lose the same frames.
    pub seed: u64,
}

impl LossProfile {
    /// The transport as the game runs it: nothing is dropped on purpose.
    pub const NONE: Self = Self {
        datagram_percent: 0,
        seed: 0,
    };

    /// Whether this profile ever drops anything.
    #[must_use]
    pub const fn is_lossless(self) -> bool {
        self.datagram_percent == 0
    }
}

/// A bound endpoint waiting for a match to be played on it.
#[derive(Debug)]
pub struct Listener {
    endpoint: Endpoint,
    certificate: Vec<u8>,
}

/// What arrives at the loop from the outside.
#[derive(Debug)]
enum Event {
    /// A connection that has opened its stream and is asking for a seat.
    Arrived(Connection, SendStream, RecvStream),
    /// One frame from a seated session, with the moment the server saw it.
    Frame(Seat, Box<[u8; CLIENT_FRAME_BYTES]>, u64),
    /// A session's stream ended.
    Closed(Seat),
}

/// One seated session's end of the wire.
#[derive(Debug)]
struct Wire {
    /// The datagrams go here; it is also what keeps the connection from being
    /// closed by being dropped.
    connection: Connection,
    /// The session's reliable half: the handshake out, every client frame in.
    send: SendStream,
    /// Frames sent to this session, which is what its shards are numbered by.
    /// A count of ticks, and therefore public: the cadence is one frame per
    /// player per tick whatever happened.
    sequence: u32,
    /// The loss this session injects, and the draws it injects it from.
    loss: LossProfile,
    losses: Rng,
    /// Datagrams this session threw away on purpose, so that a test can say
    /// what it actually did rather than what it asked for.
    dropped: u32,
}

impl Wire {
    /// Sends one frame as its shards.
    ///
    /// A refused datagram is treated as a lost one — the frame is gone, the
    /// session is not. The alternative is closing a session because the send
    /// buffer was briefly full, which hands an attacker a way to end somebody
    /// else's match by making their client stutter. The sequence advances
    /// either way, so a frame that was never sent is a gap the receiver
    /// abandons rather than a renumbering it cannot see.
    ///
    /// A shard the [`LossProfile`] drops takes the same path as one the network
    /// eats: it is not sent, nothing is told, and the sequence still advances.
    /// The draw is per shard rather than per frame, because that is what a
    /// network does and because the two produce different frame-delivery rates —
    /// which is exactly the number the exit criterion's floor is about.
    fn send_frame(&mut self, frame: &ServerFrame) {
        for shard in frame.shards(self.sequence) {
            if !self.loss.is_lossless()
                && self.losses.below(100) < u32::from(self.loss.datagram_percent.min(100))
            {
                self.dropped = self.dropped.saturating_add(1);
                continue;
            }
            let _ = self
                .connection
                .send_datagram(Bytes::copy_from_slice(shard.as_bytes()));
        }
        self.sequence = self.sequence.saturating_add(1);
    }
}

impl Listener {
    /// Binds a socket and generates the certificate this server will present.
    ///
    /// # Errors
    ///
    /// [`NetError`] if the address cannot be bound or the certificate cannot be
    /// generated.
    pub fn bind(address: SocketAddr) -> Result<Self, NetError> {
        let issued = rcgen::generate_simple_self_signed(vec!["localhost".to_owned()])
            .map_err(|error| NetError::Certificate(error.to_string()))?;
        let certificate = issued.cert.der().to_vec();
        let key = rustls::pki_types::PrivatePkcs8KeyDer::from(issued.signing_key.serialize_der());

        let config = quinn::ServerConfig::with_single_cert(
            vec![rustls::pki_types::CertificateDer::from(certificate.clone())],
            key.into(),
        )
        .map_err(|error| NetError::Certificate(error.to_string()))?;

        Ok(Self {
            endpoint: Endpoint::server(config, address)?,
            certificate,
        })
    }

    /// The address actually bound, which is what a caller that asked for port
    /// zero needs.
    ///
    /// # Errors
    ///
    /// [`NetError`] if the socket has no address.
    pub fn local_addr(&self) -> Result<SocketAddr, NetError> {
        Ok(self.endpoint.local_addr()?)
    }

    /// The DER of the certificate this server presents. Handed to clients out
    /// of band; they trust exactly this one and nothing else.
    #[must_use]
    pub fn certificate(&self) -> &[u8] {
        &self.certificate
    }

    /// Accepts sessions, runs `ticks` ticks of the match, and returns the
    /// recording.
    ///
    /// `period` is the tick period and the cadence half of the traffic-shape
    /// invariant is about it: one frame per player per period, whatever
    /// happened. The game's own rate is 30 Hz; the parameter exists because a
    /// test that had to spend thirty-three seconds of wall clock to run a
    /// thousand ticks would be a test nobody runs, and compressing the period
    /// uniformly leaves the invariant — a constant number of bytes at a constant
    /// interval — exactly as it was.
    ///
    /// # Errors
    ///
    /// [`NetError`] if a stream fails while the match is still running.
    pub async fn host(
        self,
        config: MatchConfig,
        period: Duration,
        ticks: u32,
    ) -> Result<Recording, NetError> {
        self.host_lossy(config, period, ticks, LossProfile::NONE)
            .await
    }

    /// The same, throwing away outgoing datagrams at the rate given.
    ///
    /// See [`LossProfile`] for why a server has this and why it is safe to ship.
    /// `host` is this with nothing dropped, which is what the binary calls and
    /// what the game runs.
    ///
    /// # Errors
    ///
    /// [`NetError`] if a stream fails while the match is still running.
    pub async fn host_lossy(
        self,
        config: MatchConfig,
        period: Duration,
        ticks: u32,
        loss: LossProfile,
    ) -> Result<Recording, NetError> {
        let (events, mut inbox) = mpsc::channel::<Event>(256);
        let accepting = tokio::spawn(accept_loop(self.endpoint.clone(), events.clone()));

        let mut game = Match::new(config);
        let mut wires: [Option<Wire>; PLAYER_COUNT] = [const { None }; PLAYER_COUNT];
        let mut ran = 0u32;
        let mut ticker = tokio::time::interval(period);
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

        while ran < ticks {
            tokio::select! {
                Some(event) = inbox.recv() => match event {
                    Event::Arrived(connection, mut send, recv) => {
                        let (seat, frame) = game.join();
                        send.write_all(frame.as_bytes()).await
                            .map_err(|error| NetError::Connection(error.to_string()))?;
                        match seat {
                            Some(seat) => {
                                wires[seat.index()] = Some(Wire {
                                    connection,
                                    send,
                                    sequence: 0,
                                    loss,
                                    // Offset by seat, so that two sessions do
                                    // not lose the same frames and "no two
                                    // clients disagree" is a claim about worlds
                                    // that really were delivered differently.
                                    losses: Rng::from_seed(
                                        loss.seed ^ (seat.index() as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15),
                                    ),
                                    dropped: 0,
                                });
                                tokio::spawn(read_loop(seat, recv, events.clone()));
                            }
                            // Refused, and told why. The frame is already sent;
                            // dropping the connection closes it.
                            None => {
                                let _ = send.finish();
                            }
                        }
                    }
                    Event::Frame(seat, bytes, at) => {
                        match game.deliver(seat, bytes.as_slice(), at) {
                            Ok(()) => {}
                            // A sequence number that went backwards or a seat
                            // over its rate: already counted, already dropped.
                            // Closing the session for either would hand an
                            // attacker a way to end somebody else's match by
                            // making their client stutter.
                            Err(Violation::Sequence | Violation::RateLimit) => {}
                            // A frame that is not a frame, or a message this
                            // session had no business sending. There is no
                            // benign explanation, so the session ends.
                            Err(Violation::Frame(_) | Violation::OutOfOrder) => {
                                wires[seat.index()] = None;
                            }
                        }
                    }
                    Event::Closed(seat) => {
                        wires[seat.index()] = None;
                    }
                },
                _ = ticker.tick() => {
                    let frames = game.tick();
                    if frames.is_empty() && !game.started() {
                        continue;
                    }
                    for (seat, frame) in frames {
                        let Some(wire) = wires[seat.index()].as_mut() else {
                            continue;
                        };
                        wire.send_frame(&frame);
                    }
                    ran = ran.saturating_add(1);
                }
            }
        }

        // Shutdown. `Endpoint::close` sends a connection-close frame
        // *immediately* and discards whatever has not gone out yet, so closing
        // the moment the last tick ran would throw away the frames of that
        // tick. Finish the streams so a client reading one sees a clean end,
        // give the driver a bounded moment to flush the datagrams already
        // queued, and only then close.
        //
        // The pause is what tells the clients the match is over: they are
        // waiting on datagrams, not on the stream, so the connection closing
        // under them is the signal. That is deliberate — a client whose server
        // has stopped sending state has nothing to wait for.
        for wire in wires.iter_mut().flatten() {
            let _ = wire.send.finish();
        }
        if !loss.is_lossless() {
            let dropped: u32 = wires
                .iter()
                .flatten()
                .map(|wire| wire.dropped)
                .fold(0, u32::saturating_add);
            println!(
                "server: dropped {dropped} datagrams on purpose at {}%",
                loss.datagram_percent
            );
        }
        tokio::time::sleep(DRAIN).await;
        accepting.abort();
        self.endpoint.close(0u32.into(), b"match over");
        Ok(game.recording())
    }
}

/// Accepts connections, and lets through the ones that ask for a seat properly.
///
/// The first frame on a session's stream must be `Join`, and it is consumed
/// here. That is the handshake, and putting it in this task rather than in the
/// main loop is not an arrangement of convenience: the alternative — treat the
/// arrival of a stream as the request and let the reader deliver whatever is on
/// it — feeds the `Join` frame back into `Match::deliver`, which correctly
/// refuses a second `Join` on an established session and closes it. The session
/// would then be torn down by its own opening move. Exactly one place reads the
/// first frame, and this is it.
async fn accept_loop(endpoint: Endpoint, events: mpsc::Sender<Event>) {
    while let Some(incoming) = endpoint.accept().await {
        let events = events.clone();
        tokio::spawn(async move {
            let Ok(connection) = incoming.await else {
                return;
            };
            let Ok((send, mut recv)) = connection.accept_bi().await else {
                return;
            };
            // A session that opens a stream and says something other than
            // "join" is not a session. It is dropped without an answer: there
            // is nothing to tell it that it does not already know, and a server
            // that replies to malformed openings is a server that can be made
            // to send.
            let mut opening = [0u8; CLIENT_FRAME_BYTES];
            if recv.read_exact(&mut opening).await.is_err() {
                return;
            }
            if protocol::ClientFrame::decode(&opening) != Ok(protocol::ClientMessage::Join) {
                return;
            }
            let _ = events.send(Event::Arrived(connection, send, recv)).await;
        });
    }
}

/// Reads fixed-size frames off one session's stream.
///
/// By length rather than by delimiter, which the fixed frame size makes
/// possible and which is the only framing that cannot be desynchronised by a
/// sender: there is no length field to lie in and no terminator to inject.
async fn read_loop(seat: Seat, mut recv: RecvStream, events: mpsc::Sender<Event>) {
    loop {
        let mut bytes = Box::new([0u8; CLIENT_FRAME_BYTES]);
        if recv.read_exact(bytes.as_mut_slice()).await.is_err() {
            let _ = events.send(Event::Closed(seat)).await;
            return;
        }
        if events
            .send(Event::Frame(seat, bytes, now_ms()))
            .await
            .is_err()
        {
            return;
        }
    }
}
