use std::io::{Read, Write};
use std::net::TcpStream;
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::consts::*;
use crate::error::{Error, Result};
use crate::messages::{
    create_heartbeat_ack_raw, create_heartbeat_ping_raw, create_selective_ack_raw,
    create_stream_ack_raw,
};
use native_tls::{TlsConnector, TlsStream};

/// What the client sends back once a message from the server is handled.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Reply {
    /// Acknowledge every stanza received so far (a stream ack).
    pub stream_ack: bool,
    /// Acknowledge these persistent ids by name (a selective ack).
    pub selective_ack: Vec<String>,
}

type OnMessage = Arc<dyn Fn(u8, Vec<u8>) -> Result<Reply> + Send + Sync>;

pub struct SocketHandler {
    socket: Option<TlsStream<TcpStream>>,
    heartbeat_interval: Duration,
    on_message: Option<OnMessage>,
    buffer: Vec<u8>,
    state: u8,
    message_tag: u8,
    message_size: usize,
    last_heartbeat: Instant,
    /// Stanzas received from the server on this connection, the login
    /// response first: every stanza sent back carries it as
    /// `last_stream_id_received`, which acknowledges them.
    stream_id_in: i32,
    /// When a heartbeat ping went unanswered since: nothing received since.
    ping_unanswered: Option<Instant>,
}

impl SocketHandler {
    pub fn new() -> Self {
        Self {
            socket: None,
            heartbeat_interval: Duration::from_secs(600),
            on_message: None,
            buffer: Vec::new(),
            state: MCS_VERSION_TAG_AND_SIZE,
            message_tag: 0,
            message_size: 0,
            last_heartbeat: Instant::now(),
            stream_id_in: 0,
            ping_unanswered: None,
        }
    }

    pub fn connect(&mut self) -> Result<()> {
        let connector = TlsConnector::new()
            .map_err(|e| Error::Other(format!("Failed to create TLS connector: {}", e)))?;

        let stream = TcpStream::connect(FCM_SOCKET_ADDRESS).map_err(|e| {
            Error::Other(format!(
                "Failed to connect to {}: {}",
                FCM_SOCKET_ADDRESS, e
            ))
        })?;

        stream
            .set_nodelay(true)
            .map_err(|e| Error::Other(format!("Failed to set TCP_NODELAY: {}", e)))?;
        // Reads give up now and then, so heartbeats go out on time and a
        // connection that silently died is noticed.
        stream
            .set_read_timeout(Some(Duration::from_secs(SOCKET_READ_TICK_SECS)))
            .map_err(|e| Error::Other(format!("Failed to set the read timeout: {}", e)))?;

        let tls_stream = connector
            .connect("mtalk.google.com", stream)
            .map_err(|e| Error::Other(format!("Failed to establish TLS connection: {}", e)))?;

        self.socket = Some(tls_stream);
        self.init();

        Ok(())
    }

    pub fn start_socket_handler(&mut self) -> Result<()> {
        loop {
            while self.process_state_step()? {}
            self.maybe_send_periodic_heartbeat()?;
            self.read_from_socket()?;
        }
    }

    fn read_from_socket(&mut self) -> Result<()> {
        let mut buffer = [0u8; 32 * 1024];
        let socket = self.socket_mut()?;
        let bytes_read = match socket.read(&mut buffer) {
            Ok(n) => n,
            // The read timed out: nothing arrived for a while.
            Err(e)
                if matches!(
                    e.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) =>
            {
                return match self.ping_unanswered {
                    Some(since) if since.elapsed() >= Duration::from_secs(HEARTBEAT_TIMEOUT_SECS) => {
                        Err(Error::Other("No answer to a heartbeat".to_string()))
                    }
                    _ => Ok(()),
                };
            }
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => return Ok(()),
            Err(e) => return Err(Error::Other(format!("Failed to read from socket: {}", e))),
        };

        if bytes_read == 0 {
            return Err(Error::Other("Connection closed by peer".to_string()));
        }

        // Anything at all shows the connection is alive.
        self.ping_unanswered = None;
        self.buffer.extend_from_slice(&buffer[..bytes_read]);
        Ok(())
    }

    fn process_state_step(&mut self) -> Result<bool> {
        match self.state {
            MCS_VERSION_TAG_AND_SIZE => {
                if self.buffer.len() < VERSION_PACKET_LEN {
                    return Ok(false);
                }
                let version = self.buffer.remove(0);
                if version < KMCS_VERSION && version != 38 {
                    return Err(Error::Other(format!("Invalid version: {}", version)));
                }
                self.state = MCS_TAG_AND_SIZE;
                Ok(true)
            }
            MCS_TAG_AND_SIZE => {
                if self.buffer.len() < TAG_PACKET_LEN {
                    return Ok(false);
                }
                self.message_tag = self.buffer.remove(0);
                self.state = MCS_SIZE;
                Ok(true)
            }
            MCS_SIZE => match Self::try_read_varint(&self.buffer)? {
                Some((size, consumed)) => {
                    self.buffer.drain(0..consumed);
                    self.message_size = size;
                    if self.message_size == 0 {
                        self.dispatch_message(Vec::new())?;
                        self.state = MCS_TAG_AND_SIZE;
                    } else {
                        self.state = MCS_PROTO_BYTES;
                    }
                    Ok(true)
                }
                None => Ok(false),
            },
            MCS_PROTO_BYTES => {
                if self.buffer.len() < self.message_size {
                    return Ok(false);
                }
                let payload: Vec<u8> = self.buffer.drain(0..self.message_size).collect();
                self.dispatch_message(payload)?;
                self.state = MCS_TAG_AND_SIZE;
                Ok(true)
            }
            _ => Err(Error::Other(format!(
                "Socket handler reached unexpected state ({})",
                self.state
            ))),
        }
    }

    fn dispatch_message(&mut self, payload: Vec<u8>) -> Result<()> {
        self.stream_id_in = self.stream_id_in.wrapping_add(1);
        // The server's ping is answered with an ack, which acknowledges
        // what was received too.
        if self.message_tag == K_HEARTBEAT_PING_TAG {
            self.send_heartbeat_ack()?;
        }

        let reply = match self.on_message {
            Some(ref callback) => callback(self.message_tag, payload)?,
            None => Reply::default(),
        };
        if !reply.selective_ack.is_empty() {
            let packet = create_selective_ack_raw(&reply.selective_ack, self.stream_id_in)?;
            self.send(&packet, "selective ack")?;
        }
        if reply.stream_ack {
            let packet = create_stream_ack_raw(self.stream_id_in)?;
            self.send(&packet, "stream ack")?;
        }

        self.message_tag = 0;
        self.message_size = 0;
        Ok(())
    }

    fn maybe_send_periodic_heartbeat(&mut self) -> Result<()> {
        if self.heartbeat_interval.as_secs() == 0 {
            return Ok(());
        }

        if self.last_heartbeat.elapsed() >= self.heartbeat_interval {
            self.send_heartbeat_ping()?;
            self.last_heartbeat = Instant::now();
            self.ping_unanswered.get_or_insert_with(Instant::now);
        }

        Ok(())
    }

    fn socket_mut(&mut self) -> Result<&mut TlsStream<TcpStream>> {
        self.socket
            .as_mut()
            .ok_or_else(|| Error::Other("Socket not connected".to_string()))
    }

    fn try_read_varint(data: &[u8]) -> Result<Option<(usize, usize)>> {
        let mut result = 0usize;
        let mut shift = 0usize;

        for (idx, &byte) in data.iter().enumerate().take(SIZE_PACKET_LEN_MAX) {
            result |= ((byte & 0x7F) as usize) << shift;

            if byte & 0x80 == 0 {
                return Ok(Some((result, idx + 1)));
            }

            shift += 7;
        }

        if data.len() >= SIZE_PACKET_LEN_MAX {
            return Err(Error::Other("Invalid varint encoding".to_string()));
        }

        Ok(None)
    }

    fn send_heartbeat_ping(&mut self) -> Result<()> {
        let packet = create_heartbeat_ping_raw(self.stream_id_in)?;
        self.send(&packet, "heartbeat ping")
    }

    fn send_heartbeat_ack(&mut self) -> Result<()> {
        let packet = create_heartbeat_ack_raw(self.stream_id_in)?;
        self.send(&packet, "heartbeat ack")
    }

    fn send(&mut self, packet: &[u8], what: &str) -> Result<()> {
        let socket = self.socket_mut()?;
        socket
            .write_all(packet)
            .map_err(|e| Error::Other(format!("Failed to send {}: {}", what, e)))?;
        socket
            .flush()
            .map_err(|e| Error::Other(format!("Failed to flush socket: {}", e)))?;
        Ok(())
    }

    /// Stanzas received on this connection so far (see `stream_id_in`).
    pub fn stream_id_in(&self) -> i32 {
        self.stream_id_in
    }

    pub fn send_login_handshake(&mut self, login_request: &[u8]) -> Result<()> {
        let socket = self.socket_mut()?;
        socket
            .write_all(login_request)
            .map_err(|e| Error::Other(format!("Failed to send login handshake: {}", e)))?;
        socket
            .flush()
            .map_err(|e| Error::Other(format!("Failed to flush socket: {}", e)))?;
        Ok(())
    }

    pub fn close(&mut self) {
        if let Some(mut socket) = self.socket.take() {
            let _ = socket.shutdown();
        }
        self.init();
    }

    pub fn set_on_message<F>(&mut self, callback: F)
    where
        F: Fn(u8, Vec<u8>) -> Result<()> + Send + Sync + 'static,
    {
        self.on_message = Some(Arc::new(move |tag, data| {
            callback(tag, data).map(|()| Reply::default())
        }));
    }

    /// Like `set_on_message`, with what to acknowledge once each message is
    /// handled.
    pub fn set_on_message_with_reply<F>(&mut self, callback: F)
    where
        F: Fn(u8, Vec<u8>) -> Result<Reply> + Send + Sync + 'static,
    {
        self.on_message = Some(Arc::new(callback));
    }

    pub fn set_heartbeat_interval(&mut self, interval: Duration) {
        self.heartbeat_interval = interval;
    }

    fn init(&mut self) {
        self.buffer.clear();
        self.state = MCS_VERSION_TAG_AND_SIZE;
        self.message_tag = 0;
        self.message_size = 0;
        self.last_heartbeat = Instant::now();
        self.stream_id_in = 0;
        self.ping_unanswered = None;
    }
}

impl Drop for SocketHandler {
    fn drop(&mut self) {
        self.close();
    }
}
