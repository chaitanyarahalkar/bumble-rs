use crate::{Error, PacketFramer, PacketSink, PacketSource, Result};
use bumble_hci::HciPacket;
use std::collections::VecDeque;
use std::net::{SocketAddr, TcpListener, TcpStream, ToSocketAddrs};
use tungstenite::stream::MaybeTlsStream;
use tungstenite::{HandshakeError, Message, WebSocket};

enum Connection {
    Client(Box<WebSocket<MaybeTlsStream<TcpStream>>>),
    Server(Box<WebSocket<TcpStream>>),
}

impl Connection {
    fn read(&mut self) -> tungstenite::Result<Message> {
        match self {
            Self::Client(connection) => connection.read(),
            Self::Server(connection) => connection.read(),
        }
    }

    fn send(&mut self, message: Message) -> tungstenite::Result<()> {
        match self {
            Self::Client(connection) => connection.send(message),
            Self::Server(connection) => connection.send(message),
        }
    }

    fn flush(&mut self) -> tungstenite::Result<()> {
        match self {
            Self::Client(connection) => connection.flush(),
            Self::Server(connection) => connection.flush(),
        }
    }

    fn close(&mut self) -> tungstenite::Result<()> {
        match self {
            Self::Client(connection) => connection.close(None),
            Self::Server(connection) => connection.close(None),
        }
    }
}

/// HCI packets carried in binary WebSocket messages.
pub struct WebSocketTransport {
    connection: Connection,
    framer: PacketFramer,
    pending: VecDeque<HciPacket>,
}

impl WebSocketTransport {
    pub fn connect(url: &str) -> Result<Self> {
        let (connection, _) = tungstenite::connect(url)?;
        Ok(Self::new(Connection::Client(Box::new(connection))))
    }

    fn from_server(connection: WebSocket<TcpStream>) -> Self {
        Self::new(Connection::Server(Box::new(connection)))
    }

    fn new(connection: Connection) -> Self {
        Self {
            connection,
            framer: PacketFramer::new(),
            pending: VecDeque::new(),
        }
    }

    /// Send one raw binary message. This retains Bumble's ability to carry
    /// multiple HCI packets, or a packet fragment, in one WebSocket message.
    pub fn write_binary(&mut self, bytes: impl Into<Vec<u8>>) -> Result<()> {
        self.connection.send(Message::binary(bytes.into()))?;
        Ok(())
    }

    pub fn write_text(&mut self, text: impl Into<String>) -> Result<()> {
        self.connection.send(Message::text(text.into()))?;
        Ok(())
    }

    pub fn close(&mut self) -> Result<()> {
        match self.connection.close() {
            Ok(()) | Err(tungstenite::Error::ConnectionClosed) => Ok(()),
            Err(error) => Err(error.into()),
        }
    }
}

impl PacketSource for WebSocketTransport {
    fn read_packet(&mut self) -> Result<Option<HciPacket>> {
        if let Some(packet) = self.pending.pop_front() {
            return Ok(Some(packet));
        }
        loop {
            match self.connection.read() {
                Ok(Message::Binary(bytes)) => {
                    self.pending.extend(self.framer.feed(&bytes)?);
                    if let Some(packet) = self.pending.pop_front() {
                        return Ok(Some(packet));
                    }
                }
                Ok(Message::Close(_))
                | Err(tungstenite::Error::ConnectionClosed)
                | Err(tungstenite::Error::AlreadyClosed) => return Ok(None),
                Ok(Message::Text(_) | Message::Ping(_) | Message::Pong(_) | Message::Frame(_)) => {}
                Err(error) => return Err(error.into()),
            }
        }
    }
}

impl PacketSink for WebSocketTransport {
    fn write_packet(&mut self, packet: &HciPacket) -> Result<()> {
        self.write_binary(packet.to_bytes())
    }

    fn flush(&mut self) -> Result<()> {
        self.connection.flush()?;
        Ok(())
    }
}

pub struct WebSocketServer {
    listener: TcpListener,
}

impl WebSocketServer {
    pub fn bind(address: impl ToSocketAddrs) -> Result<Self> {
        Ok(Self {
            listener: TcpListener::bind(address)?,
        })
    }

    pub fn local_addr(&self) -> Result<SocketAddr> {
        Ok(self.listener.local_addr()?)
    }

    pub fn accept(&self) -> Result<WebSocketTransport> {
        let (stream, _) = self.listener.accept()?;
        stream.set_nodelay(true)?;
        let connection = tungstenite::accept(stream).map_err(|error| match error {
            HandshakeError::Failure(error) => Error::WebSocket(error),
            HandshakeError::Interrupted(_) => {
                Error::InvalidSpec("WebSocket handshake unexpectedly interrupted".into())
            }
        })?;
        Ok(WebSocketTransport::from_server(connection))
    }
}
