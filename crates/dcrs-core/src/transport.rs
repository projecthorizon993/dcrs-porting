//! The built-in gateway transport.
//!
//! Split so most of it is testable without a socket:
//!
//! - URL construction and `zlib-stream` length prefixes - always available.
//! - `ZlibStream` - the framing Discord uses: zlib with a 4-byte big-endian length prefix per
//!   message, one independent stream each. Behind `compression`.
//! - `WsTransport` - a `tungstenite` WebSocket speaking that framing, behind the
//!   [`GatewayTransport`] trait. Behind `websocket`.
//!
//! Any other Rust Discord library can be used instead; see [`crate::backend`].
//!
//! # Known limitation: TLS fingerprint
//!
//! `tungstenite` uses a stock TLS stack. Discord detects non-browser clients partly by their TLS and
//! HTTP/2 fingerprint, and this transport does not attempt to hide that. It is fine for development
//! and for talking to your own account; it is not hardened for daily use.
//!
//! If that matters, use a backend built for it — `discord_client_gateway` ships Chrome TLS/HTTP2
//! impersonation — and hand it to the same [`Session`](crate::session::Session). The session
//! machine does not know or care which transport it is driving.

// Named in doc comments, and referenced by the websocket-gated code below. The doc-only import
// covers the feature-off case; the real one covers the feature-on case.
#[cfg(all(doc, not(feature = "websocket")))]
use crate::gateway::{GatewayTransport, Inbound};

#[cfg(feature = "compression")]
use std::io::Write;

#[cfg(feature = "websocket")]
use crate::gateway::{GatewayTransport, Inbound};
#[cfg(any(feature = "compression", feature = "websocket"))]
use crate::gateway::{Outbound, TransportError};

/// The gateway URL used when discovery is not performed.
pub const DEFAULT_GATEWAY_URL: &str = "wss://gateway.discord.gg/";

/// The gateway API version this crate speaks.
pub const GATEWAY_VERSION: u8 = 9;

/// Builds the connect URL for a gateway host.
///
/// `encoding=json` because [`Inbound`] is JSON. A URL from `GET /gateway` may or may not carry
/// its own query string; the parameters are merged either way.
#[must_use]
pub fn build_url(host: &str, encoding: &str, compression: &str) -> String {
    let base = host.trim_end_matches('/');
    // Strip any query the discovered URL already carries; ours must win.
    let base = base.split('?').next().unwrap_or(base);
    format!("{base}/?v={GATEWAY_VERSION}&encoding={encoding}&compress={compression}")
}

/// Builds the URL with the settings this crate supports.
#[must_use]
pub fn default_url(host: &str) -> String {
    build_url(host, "json", "zlib-stream")
}

/// Reads the 4-byte big-endian length prefix `zlib-stream` puts on every message.
#[must_use]
pub fn read_length_prefix(data: &[u8]) -> Option<u32> {
    let bytes: [u8; 4] = data.get(..4)?.try_into().ok()?;
    Some(u32::from_be_bytes(bytes))
}

/// Appends the 4-byte big-endian length prefix.
pub fn write_length_prefix(len: usize, out: &mut Vec<u8>) {
    out.extend_from_slice(&(len as u32).to_be_bytes());
}

/// zlib compression for Discord's `zlib-stream` framing.
///
/// Each message is its own independent zlib stream rather than one continuous stream, so a fresh
/// decoder is built per message. That is also why the type holds no state.
#[cfg(feature = "compression")]
#[derive(Debug, Default)]
pub struct ZlibStream;

#[cfg(feature = "compression")]
impl ZlibStream {
    /// Creates a decompressor.
    #[must_use]
    pub fn new() -> Self {
        Self
    }

    /// Decompresses one message.
    ///
    /// # Errors
    /// Returns [`TransportError::Decode`] if the payload is not a valid zlib stream.
    pub fn decode(&mut self, data: &[u8]) -> Result<Vec<u8>, TransportError> {
        use flate2::read::ZlibDecoder;
        use std::io::Read;

        let mut out = Vec::with_capacity(data.len() * 4);
        ZlibDecoder::new(data)
            .read_to_end(&mut out)
            .map_err(|e| TransportError::Decode(format!("zlib: {e}")))?;
        Ok(out)
    }

    /// Compresses one message. Used by tests and replay tooling; a client never sends zlib.
    ///
    /// # Errors
    /// Returns [`TransportError::Send`] if compression fails.
    pub fn encode(&mut self, data: &[u8]) -> Result<Vec<u8>, TransportError> {
        use flate2::write::ZlibEncoder;
        let mut encoder = ZlibEncoder::new(Vec::new(), flate2::Compression::default());
        encoder
            .write_all(data)
            .and_then(|()| encoder.finish())
            .map_err(|e| TransportError::Send(format!("zlib: {e}")))
    }
}

/// A blocking WebSocket gateway transport.
#[cfg(feature = "websocket")]
#[derive(Debug)]
pub struct WsTransport {
    socket:
        Option<tungstenite::WebSocket<tungstenite::stream::MaybeTlsStream<std::net::TcpStream>>>,
    zlib: ZlibStream,
}

#[cfg(feature = "websocket")]
impl Default for WsTransport {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(feature = "websocket")]
impl WsTransport {
    /// Creates a disconnected transport.
    #[must_use]
    pub fn new() -> Self {
        Self {
            socket: None,
            zlib: ZlibStream::new(),
        }
    }

    /// The URL this transport would connect to for a given gateway host.
    #[must_use]
    pub fn url(host: &str) -> String {
        default_url(host)
    }

    /// Whether a connection is currently open.
    #[must_use]
    pub fn is_connected(&self) -> bool {
        self.socket.is_some()
    }

    /// Renders an outbound frame as the JSON payload the gateway expects.
    fn wire_payload(frame: &Outbound) -> Option<serde_json::Value> {
        let (op, data) = match frame {
            Outbound::Identify {
                token,
                properties,
                client_build_number,
                capabilities,
            } => (
                2u8,
                serde_json::json!({
                    "token": token,
                    "properties": properties,
                    "client_build_number": client_build_number,
                    "capabilities": capabilities
                }),
            ),
            Outbound::Resume {
                token,
                session_id,
                seq,
            } => (
                6,
                serde_json::json!({ "token": token, "session_id": session_id, "seq": seq }),
            ),
            Outbound::Heartbeat { seq } => (1, serde_json::json!(seq)),
            Outbound::PresenceUpdate { presence } => (8, presence.clone()),
            // A Close request means "end the session"; the caller closes the socket.
            Outbound::Close { .. } => return None,
        };
        // Discord sends `s` only on Dispatch, and omits it entirely otherwise.
        Some(serde_json::json!({ "op": op, "d": data }))
    }
}

#[cfg(feature = "websocket")]
impl GatewayTransport for WsTransport {
    fn connect(&mut self, url: &str) -> Result<Inbound, TransportError> {
        let (socket, _response) = tungstenite::connect(url)
            .map_err(|e| TransportError::Connect(format!("{url}: {e}")))?;
        self.socket = Some(socket);
        // The gateway speaks first: Hello arrives without being asked for.
        self.next_frame()
    }

    fn next_frame(&mut self) -> Result<Inbound, TransportError> {
        let socket = self
            .socket
            .as_mut()
            .ok_or_else(|| TransportError::Disconnected("not connected".to_owned()))?;

        loop {
            let message = socket
                .read()
                .map_err(|e| TransportError::Disconnected(e.to_string()))?;

            // Whether the server compresses is negotiated at the protocol level, so accept either a
            // plain JSON payload or a zlib-stream one.
            //
            // Text frames are parsed straight out of the buffer tungstenite already holds: copying
            // every inbound frame into a fresh `Vec` first doubled the peak allocation on the hottest
            // path in the process, for a buffer that is read once and then dropped anyway.
            let bytes: &[u8] = match &message {
                tungstenite::Message::Text(t) => t.as_bytes(),
                tungstenite::Message::Binary(b) => b,
                tungstenite::Message::Close(frame) => {
                    let code = frame.as_ref().map_or(1000u16, |f| u16::from(f.code));
                    return Err(TransportError::Disconnected(format!("closed {code}")));
                }
                // Ping/pong are answered by tungstenite; nothing to decode.
                _ => continue,
            };

            let json = if let Ok(value) = serde_json::from_slice::<serde_json::Value>(bytes) {
                value
            } else {
                let inflated = self.zlib.decode(bytes)?;
                serde_json::from_slice(&inflated)
                    .map_err(|e| TransportError::Decode(e.to_string()))?
            };

            return serde_json::from_value(json).map_err(|e| TransportError::Decode(e.to_string()));
        }
    }

    fn send(&mut self, frame: &Outbound) -> Result<(), TransportError> {
        let Some(payload) = Self::wire_payload(frame) else {
            return Ok(());
        };
        let socket = self
            .socket
            .as_mut()
            .ok_or_else(|| TransportError::Send("not connected".to_owned()))?;
        socket
            .send(tungstenite::Message::Text(payload.to_string().into()))
            .map_err(|e| TransportError::Send(e.to_string()))
    }

    fn close(&mut self) {
        if let Some(socket) = &mut self.socket {
            let _ = socket.close(None);
        }
        self.socket = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn url_carries_version_encoding_and_compression() {
        let url = default_url("wss://gateway.discord.gg");
        assert!(url.starts_with("wss://gateway.discord.gg/?"));
        assert!(url.contains("v=9"), "{url}");
        assert!(url.contains("encoding=json"), "{url}");
        assert!(url.contains("compress=zlib-stream"), "{url}");
    }

    #[test]
    fn url_tolerates_a_trailing_slash() {
        assert_eq!(
            default_url("wss://gateway.discord.gg/"),
            default_url("wss://gateway.discord.gg")
        );
    }

    #[test]
    fn discovered_query_parameters_are_replaced_not_duplicated() {
        // `GET /gateway` sometimes returns a URL with its own query string.
        let url = default_url("wss://gateway.discord.gg/?v=9&encoding=json");
        assert_eq!(
            url.matches("encoding=").count(),
            1,
            "encoding was duplicated: {url}"
        );
        assert!(url.contains("compress=zlib-stream"), "{url}");
    }

    #[test]
    fn length_prefix_is_big_endian() {
        let mut out = Vec::new();
        write_length_prefix(258, &mut out);
        assert_eq!(out, vec![0, 0, 1, 2]);
        assert_eq!(read_length_prefix(&out), Some(258));
        assert_eq!(
            read_length_prefix(&[0, 0]),
            None,
            "short buffers must not panic"
        );
    }

    #[cfg(feature = "compression")]
    #[test]
    fn zlib_stream_round_trips() {
        let mut encoder = ZlibStream::new();
        let mut decoder = ZlibStream::new();
        let original = br#"{"op":0,"t":"MESSAGE_CREATE","d":{"content":"hello"}}"#;
        let compressed = encoder.encode(original).unwrap();
        assert_eq!(decoder.decode(&compressed).unwrap(), original);
    }

    #[cfg(feature = "compression")]
    #[test]
    fn zlib_stream_resets_between_messages() {
        // A single continuous decompressor would fail here; `zlib-stream` is one stream per message.
        let mut encoder = ZlibStream::new();
        let mut decoder = ZlibStream::new();
        for i in 0..3 {
            let message = format!(r#"{{"n":{i}}}"#);
            let compressed = encoder.encode(message.as_bytes()).unwrap();
            assert_eq!(decoder.decode(&compressed).unwrap(), message.as_bytes());
        }
    }

    #[cfg(feature = "compression")]
    #[test]
    fn zlib_stream_rejects_garbage() {
        let mut decoder = ZlibStream::new();
        assert!(decoder.decode(&[0xff, 0xfe, 0xfd, 0xfc]).is_err());
    }

    #[cfg(feature = "compression")]
    #[test]
    fn zlib_stream_handles_a_large_payload() {
        let mut encoder = ZlibStream::new();
        let mut decoder = ZlibStream::new();
        let big = "x".repeat(100_000);
        let compressed = encoder.encode(big.as_bytes()).unwrap();
        assert!(
            compressed.len() < big.len(),
            "repetitive input should compress"
        );
        assert_eq!(decoder.decode(&compressed).unwrap(), big.as_bytes());
    }

    #[cfg(feature = "websocket")]
    #[test]
    fn url_helper_matches_the_free_function() {
        assert_eq!(
            WsTransport::url("wss://gateway.discord.gg"),
            default_url("wss://gateway.discord.gg")
        );
    }

    #[cfg(feature = "websocket")]
    #[test]
    fn outbound_frames_render_the_expected_opcodes() {
        use crate::gateway::IdentifyConfig;

        let identify =
            WsTransport::wire_payload(&IdentifyConfig::new("tok", 1).to_outbound()).unwrap();
        assert_eq!(identify["op"], 2);
        assert_eq!(identify["d"]["token"], "tok");

        let heartbeat = WsTransport::wire_payload(&Outbound::Heartbeat { seq: Some(4) }).unwrap();
        assert_eq!(heartbeat["op"], 1);
        assert_eq!(heartbeat["d"], 4);

        let resume = WsTransport::wire_payload(&Outbound::Resume {
            token: "t".into(),
            session_id: "s".into(),
            seq: 9,
        })
        .unwrap();
        assert_eq!(resume["op"], 6);
        assert_eq!(resume["d"]["session_id"], "s");

        assert!(
            WsTransport::wire_payload(&Outbound::Close {
                code: 1000,
                reason: "bye".into()
            })
            .is_none()
        );
    }

    #[cfg(feature = "websocket")]
    #[test]
    fn a_fresh_transport_is_not_connected() {
        use crate::gateway::GatewayTransport;

        let mut transport = WsTransport::new();
        assert!(!transport.is_connected());
        // Reading or writing before connecting must fail cleanly rather than panic.
        assert!(transport.next_frame().is_err());
        assert!(transport.send(&Outbound::Heartbeat { seq: None }).is_err());
    }
}
