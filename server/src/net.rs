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
//! # Streams rather than datagrams, and the reason is the padding budget
//!
//! A `ServerFrame` is 1501 bytes. A QUIC datagram is bounded by the path MTU,
//! which in practice is around 1200 bytes, so the padded frame does not fit in
//! one and datagrams are out. The frames therefore travel on one bidirectional
//! stream per session, back to back, and are read by length — every frame is
//! the same length, so there is no delimiter to get wrong and no length prefix
//! to leak anything.
//!
//! The traffic-shape invariant survives that unharmed, and it is worth saying
//! why rather than assuming it: a constant number of bytes emitted at a constant
//! period is packetised by QUIC into a number of packets that is a function of
//! the byte count and the MTU, both of which are constants. What an observer
//! counts is still a constant. The cost is head-of-line blocking on a lost
//! packet, which is real netcode and the wrong trade for a shipped MOBA — and
//! it is the honest consequence of a bucket sized for the worst case rather
//! than for the wire. `docs/ARCHITECTURE.md` records it under the padding
//! budget.
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

use protocol::CLIENT_FRAME_BYTES;
use quinn::{Connection, Endpoint, RecvStream, SendStream};
use replay::Recording;
use sim::{PLAYER_COUNT, Seat};
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

/// How long the shutdown waits for a peer to read what it has already been
/// sent. Bounded, because a client that has stopped reading must not be able to
/// hold the server open.
const DRAIN: Duration = Duration::from_secs(5);

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
    /// Held so the connection is not closed by being dropped.
    _connection: Connection,
    send: SendStream,
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
                                wires[seat.index()] = Some(Wire { _connection: connection, send });
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
                        if wire.send.write_all(frame.as_bytes()).await.is_err() {
                            wires[seat.index()] = None;
                        }
                    }
                    ran = ran.saturating_add(1);
                }
            }
        }

        // Shutdown, in the one order that does not throw the last ticks away.
        // `Endpoint::close` sends a connection-close frame *immediately* and
        // discards whatever is still unacknowledged, so closing before the
        // peers have read leaves them with a stream that ended after zero
        // bytes. Finish each stream, wait for the peer to have read it, and
        // only then close.
        //
        // Every wait is bounded. A client that has stopped reading is a client
        // that has crashed or is misbehaving, and a server that waits on one
        // forever is a server one client can hang.
        for wire in wires.iter_mut().flatten() {
            let _ = wire.send.finish();
        }
        for wire in wires.iter_mut().flatten() {
            let _ = tokio::time::timeout(DRAIN, wire.send.stopped()).await;
        }
        accepting.abort();
        let _ = tokio::time::timeout(DRAIN, self.endpoint.wait_idle()).await;
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
