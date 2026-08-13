//! The client's end of the transport.
//!
//! Thin on purpose. Everything that decides anything lives in [`Headless`],
//! which is a state machine over frames with no clock and no socket; this
//! module turns a socket into frames and back. That split is what lets the
//! exploit suite at M7 drive the same protocol from a client that shares none
//! of this code, which is `docs/ARCHITECTURE.md`'s reason for `cheat-client`
//! depending on `protocol` alone.
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

use protocol::{ClientFrame, SERVER_FRAME_BYTES};
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
    /// Held so the connection is not closed by being dropped.
    _connection: Connection,
    send: SendStream,
    recv: RecvStream,
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
            _connection: connection,
            send,
            recv,
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

    /// Reads one frame, by length.
    ///
    /// Every server frame is the same number of bytes, so there is no
    /// delimiter, no length prefix, and nothing a sender could desynchronise.
    ///
    /// # Errors
    ///
    /// [`NetError::Connection`] if the stream ends before a whole frame
    /// arrives, which is what a clean shutdown looks like from here.
    pub async fn recv(&mut self) -> Result<[u8; SERVER_FRAME_BYTES], NetError> {
        let mut bytes = [0u8; SERVER_FRAME_BYTES];
        self.recv
            .read_exact(&mut bytes)
            .await
            .map_err(|error| NetError::Connection(error.to_string()))?;
        Ok(bytes)
    }
}
