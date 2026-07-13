//! External HCI transports ported from `google/bumble`.
//!
//! Bumble transports carry complete H4 packets, including their leading packet
//! type byte. [`PacketFramer`] handles arbitrarily fragmented or coalesced byte
//! streams, while [`H4Transport`] adapts any blocking `Read + Write` object.
//! File, TCP, UDP, and Unix-domain socket endpoints build on that common layer.

mod android_emulator;
mod android_netsim;
mod bridge;
mod command_channel;
mod common;
mod dispatch;
mod file;
mod hci_socket;
mod host;
#[cfg(unix)]
mod pty;
mod serial;
pub mod snoop;
mod tcp;
mod udp;
#[cfg(unix)]
mod unix;
mod usb;
mod vhci;
mod websocket;

pub use android_emulator::{
    android_emulator_proto, AndroidEmulatorIo, AndroidEmulatorMode, AndroidEmulatorPacket,
    AndroidEmulatorPacketSink, AndroidEmulatorPacketSource, AndroidEmulatorSpec,
    AndroidEmulatorTransport, GrpcAndroidEmulatorIo, SystemAndroidEmulatorTransport,
    DEFAULT_ANDROID_EMULATOR_ADDRESS,
};
pub use android_netsim::{
    android_netsim_proto, default_netsim_ini_dir, find_netsim_grpc_port, find_netsim_grpc_port_in,
    netsim_ini_file_name, AndroidNetsimIo, AndroidNetsimMode, AndroidNetsimPacket,
    AndroidNetsimPacketSink, AndroidNetsimPacketSource, AndroidNetsimSpec, AndroidNetsimTransport,
    GrpcAndroidNetsimControllerIo, GrpcAndroidNetsimHostIo, SystemAndroidNetsimIo,
    SystemAndroidNetsimTransport, DEFAULT_ANDROID_NETSIM_MANUFACTURER, DEFAULT_ANDROID_NETSIM_NAME,
    DEFAULT_ANDROID_NETSIM_VARIANT,
};
pub use bridge::{BridgeDirection, FilteredPacket, HciBridge, PacketFilter, PacketTrace};
pub use command_channel::{CommandResponse, HciCommandChannel};
pub use common::{
    Error, H4Transport, PacketFramer, PacketLayout, PacketSink, PacketSource, Result,
    MAX_HCI_PACKET_SIZE,
};
pub use dispatch::{
    open_split_transport, open_transport, ExternalTransport, OpenedTransport, SplitOpenedTransport,
    TransportSpec,
};
pub use file::FileTransport;
pub use hci_socket::{
    HciSocketAddress, HciSocketIo, HciSocketSpec, HciSocketTransport, RawHciSocket,
    SystemHciSocketTransport, HCI_CHANNEL_USER,
};
pub use host::{
    ClassicCtkdPairingSession, ClassicPairingSession, ExternalAttTransport, ExternalControllerInfo,
    ExternalHost, ExternalHostActivity, ExternalHostState, LePairingSession,
};
#[cfg(unix)]
pub use pty::PtyTransport;
pub use serial::{SerialConfig, SerialTransport, DEFAULT_POST_OPEN_DELAY, DEFAULT_SERIAL_SPEED};
pub use snoop::{
    BtSnoopReader, BtSnoopRecord, BtSnooper, FileSnooper, PcapSnooper, SnoopDataLinkType,
    SnoopDirection, Snooper, SnooperFormat, SnooperIoType, SnooperSpec, SnoopingTransport,
};
pub use tcp::{TcpServer, TcpTransport};
pub use udp::UdpTransport;
#[cfg(unix)]
pub use unix::{UnixServer, UnixTransport};
pub use usb::{
    select_interface_layout, SystemUsbTransport, UsbEndpointInfo, UsbInterfaceInfo,
    UsbInterfaceLayout, UsbIo, UsbSelector, UsbSpec, UsbTransferError, UsbTransport,
};
pub use vhci::{VhciTransport, HCI_BREDR, HCI_VENDOR_PACKET};
pub use websocket::{
    WebSocketPacketSink, WebSocketPacketSource, WebSocketServer, WebSocketTransport,
};
