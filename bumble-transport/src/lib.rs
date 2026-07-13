//! External HCI transports ported from `google/bumble`.
//!
//! Bumble transports carry complete H4 packets, including their leading packet
//! type byte. [`PacketFramer`] handles arbitrarily fragmented or coalesced byte
//! streams, while [`H4Transport`] adapts any blocking `Read + Write` object.
//! File, TCP, UDP, and Unix-domain socket endpoints build on that common layer.

mod common;
mod dispatch;
mod file;
mod hci_socket;
#[cfg(unix)]
mod pty;
mod serial;
mod tcp;
mod udp;
#[cfg(unix)]
mod unix;
mod usb;
mod vhci;
mod websocket;

pub use common::{
    Error, H4Transport, PacketFramer, PacketLayout, PacketSink, PacketSource, Result,
    MAX_HCI_PACKET_SIZE,
};
pub use dispatch::{open_transport, ExternalTransport, OpenedTransport, TransportSpec};
pub use file::FileTransport;
pub use hci_socket::{
    HciSocketAddress, HciSocketIo, HciSocketSpec, HciSocketTransport, RawHciSocket,
    SystemHciSocketTransport, HCI_CHANNEL_USER,
};
#[cfg(unix)]
pub use pty::PtyTransport;
pub use serial::{SerialConfig, SerialTransport, DEFAULT_POST_OPEN_DELAY, DEFAULT_SERIAL_SPEED};
pub use tcp::{TcpServer, TcpTransport};
pub use udp::UdpTransport;
#[cfg(unix)]
pub use unix::{UnixServer, UnixTransport};
pub use usb::{
    select_interface_layout, SystemUsbTransport, UsbEndpointInfo, UsbInterfaceInfo,
    UsbInterfaceLayout, UsbIo, UsbSelector, UsbSpec, UsbTransferError, UsbTransport,
};
pub use vhci::{VhciTransport, HCI_BREDR, HCI_VENDOR_PACKET};
pub use websocket::{WebSocketServer, WebSocketTransport};
