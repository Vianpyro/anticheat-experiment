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
        endpoint.set_default_client_config(
            quinn::ClientConfig::with_root_certificates(Arc::new(roots))
                .map_err(|error| NetError::Certificate(error.to_string()))?,
        );

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
