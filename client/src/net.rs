//! The client's end of the transport.
//!
//! Thin on purpose. Everything that decides anything lives in [`Headless`],
//! which is a state machine over frames with no clock and no socket; this
//! module turns a socket into frames and back. That split is what lets the
//! exploit suite at M7 drive the same protocol from a client that shares none
//! of this code, which is `docs/ARCHITECTURE.md`'s reason for `cheat-client`
//! depending on `protocol` alone.
//!
//! # Two ways in, because state and session want different things
//!
//! The session's frames — the `Accepted` or `Rejected` that answers `Join` —
//! arrive on the bidirectional stream this client opens, because they are sent
//! once and have to arrive. Every `View` arrives as
//! [`protocol::SERVER_SHARDS`] datagrams, reassembled by
//! [`protocol::ShardAssembler`], because a view is better lost than late: at
//! 30 Hz a retransmission is a stall, and a client that predicts wants the next
//! tick rather than the previous one.
//!
//! The two are separate calls rather than one that selects over both, and that
//! is not an arrangement of convenience. `quinn::RecvStream::read_exact` is not
//! cancel-safe — a partially read frame is lost with the future — so a
//! `select!` over a stream read and a datagram read would silently corrupt the
//! stream the first time a datagram won the race. Since the stream carries
//! exactly one server frame per session, the handshake reads it once and
//! nothing selects over it afterwards.
//!
//! # The certificate is trusted exactly, and nothing else is
//!
//! The server generates a self-signed certificate at startup and the client is
//! handed its DER out of band. That certificate, and only that certificate, is
//! the root store. The alternative — a `ServerCertVerifier` that accepts
//! anything — is the shape of code that gets copied out of a test into a
//! release, and its absence here is deliberate rather than an oversight.
//!
//! [`Headless`]: crate::Headless

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use protocol::{ClientFrame, SERVER_FRAME_BYTES, ShardAssembler};
use quinn::{Connection, Endpoint, RecvStream, SendStream};

/// Why the transport gave up.
#[derive(Debug)]
pub enum NetError {
    /// The socket, the endpoint, or a stream.
    Io(std::io::Error),
    /// The certificate the client was handed is not one.
    Certificate(String),
    /// The connection failed or ended.
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

/// How long this client may say nothing before the transport gives up on it.
///
/// The mirror of `server::net`'s constant, and it has to be at least as large:
/// QUIC negotiates the idle timeout as the **minimum** of what the two peers
/// announce, so a generous server and a default client produce a default
/// connection.
const IDLE_TIMEOUT: Duration = Duration::from_secs(600);

/// How often this client says something when it has nothing to say.
///
/// **The lobby is a silence and it is supposed to be a long one.** Nothing
/// crosses the wire between `Ready` and the first tick — the server emits no
/// frame until every occupied seat is ready, and this client sends an intention
/// only in answer to a frame — so at quinn's default of no keep-alive and a
/// thirty-second idle timeout, every session died half a minute into the wait
/// the lobby exists to fill. What that looked like from the outside was nothing
/// at all: the bots reported no views and exited cleanly, because a closed
/// connection is how a match ends, and the click on `Ready` went into a
/// connection that had been gone for minutes.
///
/// Five seconds, which is a QUIC PING frame — a few dozen bytes, on a link this
/// project spends 268 kbit/s per player on once a match is running. It is
/// deliberately not something the game sends: an application-level heartbeat
/// would be a message whose *existence* an observer counts, and
/// `docs/ARCHITECTURE.md`'s traffic-shape invariant is about exactly that. The
/// transport's keep-alive runs only while the connection is otherwise idle,
/// which is to say only while there is nothing to observe.
const KEEP_ALIVE: Duration = Duration::from_secs(5);

/// The DER of the certificate a server printed, from the hex it printed it as.
///
/// `None` for anything that is not an even number of hex digits. It lives here
/// rather than in a binary because there are two of them now — `moba-client` and
/// `moba-bots` — and a second hand-rolled parser is a second place for the one
/// thing a client is handed out of band to be read wrongly.
#[must_use]
pub fn certificate_from_hex(text: &str) -> Option<Vec<u8>> {
    if !text.len().is_multiple_of(2) {
        return None;
    }
    (0..text.len())
        .step_by(2)
        .map(|at| u8::from_str_radix(text.get(at..at.checked_add(2)?)?, 16).ok())
        .collect()
}

/// One session's wire.
#[derive(Debug)]
pub struct Wire {
    /// The datagrams arrive here; it is also what keeps the connection from
    /// being closed by being dropped.
    connection: Connection,
    send: SendStream,
    recv: RecvStream,
    shards: ShardAssembler,
}

impl Wire {
    /// Connects to a server, trusting exactly the certificate given.
    ///
    /// # Errors
    ///
    /// [`NetError`] if the endpoint cannot be created, the certificate is not a
    /// certificate, or the connection is refused.
    pub async fn connect(address: SocketAddr, certificate: &[u8]) -> Result<Self, NetError> {
        let mut roots = rustls::RootCertStore::empty();
        roots
            .add(rustls::pki_types::CertificateDer::from(
                certificate.to_vec(),
            ))
            .map_err(|error| NetError::Certificate(error.to_string()))?;

        let mut endpoint = Endpoint::client(SocketAddr::from(([0, 0, 0, 0], 0)))?;
        let mut config = quinn::ClientConfig::with_root_certificates(Arc::new(roots))
            .map_err(|error| NetError::Certificate(error.to_string()))?;
        let mut transport = quinn::TransportConfig::default();
        transport.max_idle_timeout(Some(
            IDLE_TIMEOUT
                .try_into()
                .expect("ten minutes is inside a QUIC idle timeout"),
        ));
        transport.keep_alive_interval(Some(KEEP_ALIVE));
        config.transport_config(Arc::new(transport));
        endpoint.set_default_client_config(config);

        let connection = endpoint
            .connect(address, "localhost")
            .map_err(|error| NetError::Connection(error.to_string()))?
            .await
            .map_err(|error| NetError::Connection(error.to_string()))?;
        let (send, recv) = connection
            .open_bi()
            .await
            .map_err(|error| NetError::Connection(error.to_string()))?;

        Ok(Self {
            connection,
            send,
            recv,
            shards: ShardAssembler::new(),
        })
    }

    /// Writes one frame.
    ///
    /// # Errors
    ///
    /// [`NetError::Connection`] if the stream is gone.
    pub async fn send(&mut self, frame: &ClientFrame) -> Result<(), NetError> {
        self.send
            .write_all(frame.as_bytes())
            .await
            .map_err(|error| NetError::Connection(error.to_string()))
    }

    /// Reads the session's one frame off the stream, by length.
    ///
    /// Every server frame is the same number of bytes, so there is no
    /// delimiter, no length prefix, and nothing a sender could desynchronise.
    ///
    /// # Errors
    ///
    /// [`NetError::Connection`] if the stream ends before a whole frame
    /// arrives.
    pub async fn recv_session(&mut self) -> Result<[u8; SERVER_FRAME_BYTES], NetError> {
        let mut bytes = [0u8; SERVER_FRAME_BYTES];
        self.recv
            .read_exact(&mut bytes)
            .await
            .map_err(|error| NetError::Connection(error.to_string()))?;
        Ok(bytes)
    }

    /// Reads datagrams until one completes a frame.
    ///
    /// A shard that does not decode is discarded rather than fatal: it came off
    /// a socket, and a session that ended because somebody sprayed the port
    /// would be a session an unrelated party can end. A frame whose shard never
    /// arrives is abandoned by the assembler when the next frame starts, and
    /// [`Wire::losses`] is how a caller sees that happen.
    ///
    /// # Errors
    ///
    /// [`NetError::Connection`] when the connection ends, which is what the end
    /// of the match looks like from here.
    pub async fn recv_state(&mut self) -> Result<[u8; SERVER_FRAME_BYTES], NetError> {
        loop {
            let datagram = self
                .connection
                .read_datagram()
                .await
                .map_err(|error| NetError::Connection(error.to_string()))?;
            if let Ok(Some(frame)) = self.shards.accept(&datagram) {
                return Ok(*frame.as_bytes());
            }
        }
    }

    /// Frames abandoned for a missing shard, and shards that arrived too late
    /// to belong to one.
    ///
    /// Not an error condition: the state channel is unreliable on purpose. It
    /// is reported so that "the client missed a tick" is a number somebody can
    /// read rather than an absence they have to infer.
    #[must_use]
    pub const fn losses(&self) -> (u32, u32) {
        (self.shards.incomplete(), self.shards.stale())
    }
}

#[cfg(test)]
mod tests {
    use super::{IDLE_TIMEOUT, KEEP_ALIVE};
    use std::time::Duration;

    /// quinn's default idle timeout, which is what a peer this project did not
    /// configure announces. RFC 9308 §3.2's recommendation, and the number every
    /// session in the first playtest died at.
    const QUINN_DEFAULT_IDLE: Duration = Duration::from_secs(30);

    /// **The keep-alive has to be shorter than the shortest timeout it could
    /// meet, and that is not this project's.**
    ///
    /// A QUIC idle timeout is negotiated as the *minimum* of what the two peers
    /// announce, so a client talking to anything left at the default gets 30
    /// seconds whatever this crate asks for. A keep-alive above that would keep
    /// nothing alive, and the failure would look exactly like the one it was
    /// written to fix.
    #[test]
    fn the_keep_alive_is_shorter_than_a_default_peer_would_wait() {
        assert!(
            KEEP_ALIVE.saturating_mul(2) < QUINN_DEFAULT_IDLE,
            "a keep-alive of {KEEP_ALIVE:?} against a default idle timeout of \
             {QUINN_DEFAULT_IDLE:?} leaves no room for a lost ping"
        );
        assert!(
            KEEP_ALIVE < IDLE_TIMEOUT,
            "the keep-alive is longer than the timeout it refreshes"
        );
    }

    /// **The two ends of this project announce the same ceiling.**
    ///
    /// The minimum is what is negotiated, so a server that waits ten minutes and
    /// a client that waits thirty seconds is a connection that waits thirty
    /// seconds. `server` is a dev-dependency here, which is the allowance
    /// `docs/ARCHITECTURE.md` grants for exactly this — a claim about the two
    /// crates together that neither can state alone.
    #[test]
    fn both_ends_announce_the_same_idle_ceiling() {
        assert_eq!(
            IDLE_TIMEOUT,
            server::net::IDLE_TIMEOUT,
            "the client and the server announce different idle timeouts, so the \
             connection runs on whichever is smaller and one of the two comments \
             explaining the number is wrong"
        );
    }
}
