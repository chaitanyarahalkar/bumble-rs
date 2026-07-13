//! External HCI transports ported from `google/bumble`.
//!
//! Bumble transports carry complete H4 packets, including their leading packet
//! type byte. [`PacketFramer`] handles arbitrarily fragmented or coalesced byte
//! streams, while [`H4Transport`] adapts any blocking `Read + Write` object.
//! File, TCP, UDP, and Unix-domain socket endpoints build on that common layer.

mod common;
mod file;
mod tcp;
mod udp;
#[cfg(unix)]
mod unix;

pub use common::{
    Error, H4Transport, PacketFramer, PacketLayout, PacketSink, PacketSource, Result,
    MAX_HCI_PACKET_SIZE,
};
pub use file::FileTransport;
pub use tcp::{TcpServer, TcpTransport};
pub use udp::UdpTransport;
#[cfg(unix)]
pub use unix::{UnixServer, UnixTransport};
