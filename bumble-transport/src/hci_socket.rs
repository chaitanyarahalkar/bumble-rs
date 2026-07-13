use crate::{Error, PacketFramer, PacketSink, PacketSource, Result, MAX_HCI_PACKET_SIZE};
use bumble_hci::HciPacket;
use std::collections::VecDeque;
use std::io;

/// Linux's raw HCI user-channel number.
pub const HCI_CHANNEL_USER: u16 = 1;

/// Parsed `hci-socket:<adapter-index>` parameters.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct HciSocketSpec {
    pub adapter_index: u16,
}

impl HciSocketSpec {
    pub fn parse(parameters: Option<&str>) -> Result<Self> {
        let value = parameters.unwrap_or_default().trim();
        if value.is_empty() {
            return Ok(Self::default());
        }
        let adapter_index = value.parse::<u16>().map_err(|_| {
            Error::InvalidSpec(format!(
                "HCI socket adapter index must be an unsigned 16-bit integer: {value}"
            ))
        })?;
        Ok(Self { adapter_index })
    }
}

/// Portable representation of Linux's six-byte `sockaddr_hci` payload.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HciSocketAddress {
    pub family: u16,
    pub adapter_index: u16,
    pub channel: u16,
}

impl HciSocketAddress {
    pub const LINUX_AF_BLUETOOTH: u16 = 31;

    pub const fn user_channel(adapter_index: u16) -> Self {
        Self {
            family: Self::LINUX_AF_BLUETOOTH,
            adapter_index,
            channel: HCI_CHANNEL_USER,
        }
    }

    /// Return the native-endian bytes passed to Linux `bind(2)`.
    pub fn to_ne_bytes(self) -> [u8; 6] {
        let family = self.family.to_ne_bytes();
        let adapter = self.adapter_index.to_ne_bytes();
        let channel = self.channel.to_ne_bytes();
        [
            family[0], family[1], adapter[0], adapter[1], channel[0], channel[1],
        ]
    }
}

/// Packet-oriented I/O used by an HCI raw socket.
pub trait HciSocketIo {
    fn recv(&mut self, buffer: &mut [u8]) -> io::Result<usize>;
    fn send(&mut self, packet: &[u8]) -> io::Result<usize>;
}

/// Synchronous H4 transport over a packet-oriented HCI socket.
pub struct HciSocketTransport<B> {
    io: B,
    adapter_index: u16,
    framer: PacketFramer,
    pending: VecDeque<HciPacket>,
    receive_buffer: Box<[u8]>,
}

impl<B> HciSocketTransport<B> {
    pub fn from_io(io: B, adapter_index: u16) -> Self {
        Self {
            io,
            adapter_index,
            framer: PacketFramer::new(),
            pending: VecDeque::new(),
            receive_buffer: vec![0; MAX_HCI_PACKET_SIZE].into_boxed_slice(),
        }
    }

    pub fn adapter_index(&self) -> u16 {
        self.adapter_index
    }

    pub fn get_ref(&self) -> &B {
        &self.io
    }

    pub fn get_mut(&mut self) -> &mut B {
        &mut self.io
    }

    pub fn into_inner(self) -> B {
        self.io
    }
}

impl<B: HciSocketIo> PacketSource for HciSocketTransport<B> {
    fn read_packet(&mut self) -> Result<Option<HciPacket>> {
        if let Some(packet) = self.pending.pop_front() {
            return Ok(Some(packet));
        }

        loop {
            let count = self.io.recv(&mut self.receive_buffer)?;
            if count == 0 {
                return if self.framer.is_empty() {
                    Ok(None)
                } else {
                    Err(Error::TruncatedPacket(self.framer.buffered_len()))
                };
            }
            self.pending
                .extend(self.framer.feed(&self.receive_buffer[..count])?);
            if let Some(packet) = self.pending.pop_front() {
                return Ok(Some(packet));
            }
        }
    }
}

impl<B: HciSocketIo> PacketSink for HciSocketTransport<B> {
    fn write_packet(&mut self, packet: &HciPacket) -> Result<()> {
        let bytes = packet.to_bytes();
        let count = self.io.send(&bytes)?;
        if count != bytes.len() {
            return Err(io::Error::new(
                io::ErrorKind::WriteZero,
                format!("HCI socket sent {count} of {} bytes", bytes.len()),
            )
            .into());
        }
        Ok(())
    }
}

/// Operating-system raw HCI socket backend.
#[cfg(target_os = "linux")]
pub struct RawHciSocket {
    fd: std::os::fd::OwnedFd,
}

#[cfg(target_os = "linux")]
impl RawHciSocket {
    const BTPROTO_HCI: libc::c_int = 1;

    pub fn open(adapter_index: u16) -> io::Result<Self> {
        use std::mem::size_of;
        use std::os::fd::{AsRawFd, FromRawFd};

        #[repr(C)]
        struct SockAddrHci {
            family: libc::sa_family_t,
            adapter_index: u16,
            channel: u16,
        }

        // SAFETY: `socket` has no Rust-side aliasing requirements. Its return
        // value is checked before ownership is transferred to `OwnedFd`.
        let raw_fd = unsafe {
            libc::socket(
                libc::AF_BLUETOOTH,
                libc::SOCK_RAW | libc::SOCK_CLOEXEC,
                Self::BTPROTO_HCI,
            )
        };
        if raw_fd < 0 {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: `raw_fd` is a newly created, valid descriptor owned here.
        let fd = unsafe { std::os::fd::OwnedFd::from_raw_fd(raw_fd) };
        let portable_address = HciSocketAddress::user_channel(adapter_index);
        debug_assert_eq!(
            size_of::<SockAddrHci>(),
            portable_address.to_ne_bytes().len()
        );
        let address = SockAddrHci {
            family: portable_address.family as libc::sa_family_t,
            adapter_index: portable_address.adapter_index,
            channel: portable_address.channel,
        };
        // SAFETY: `address` is a correctly aligned `sockaddr_hci`-compatible
        // value and the supplied length is exactly its initialized size.
        let result = unsafe {
            libc::bind(
                fd.as_raw_fd(),
                (&raw const address).cast::<libc::sockaddr>(),
                size_of::<SockAddrHci>() as libc::socklen_t,
            )
        };
        if result < 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(Self { fd })
    }
}

#[cfg(target_os = "linux")]
impl HciSocketIo for RawHciSocket {
    fn recv(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        use std::os::fd::AsRawFd;

        // SAFETY: the mutable slice is valid for `buffer.len()` bytes for the
        // duration of the syscall, and the descriptor remains owned by `self`.
        let count = unsafe {
            libc::recv(
                self.fd.as_raw_fd(),
                buffer.as_mut_ptr().cast(),
                buffer.len(),
                0,
            )
        };
        if count < 0 {
            Err(io::Error::last_os_error())
        } else {
            Ok(count as usize)
        }
    }

    fn send(&mut self, packet: &[u8]) -> io::Result<usize> {
        use std::os::fd::AsRawFd;

        // SAFETY: the immutable slice is valid for `packet.len()` bytes for
        // the duration of the syscall, and the descriptor remains owned.
        let count =
            unsafe { libc::send(self.fd.as_raw_fd(), packet.as_ptr().cast(), packet.len(), 0) };
        if count < 0 {
            Err(io::Error::last_os_error())
        } else {
            Ok(count as usize)
        }
    }
}

/// Placeholder that keeps the transport API portable while opening remains
/// explicitly unsupported away from Linux.
#[cfg(not(target_os = "linux"))]
pub struct RawHciSocket;

#[cfg(not(target_os = "linux"))]
impl HciSocketIo for RawHciSocket {
    fn recv(&mut self, _buffer: &mut [u8]) -> io::Result<usize> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "raw HCI sockets require Linux",
        ))
    }

    fn send(&mut self, _packet: &[u8]) -> io::Result<usize> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "raw HCI sockets require Linux",
        ))
    }
}

pub type SystemHciSocketTransport = HciSocketTransport<RawHciSocket>;

impl SystemHciSocketTransport {
    pub fn open(parameters: Option<&str>) -> Result<Self> {
        let spec = HciSocketSpec::parse(parameters)?;
        #[cfg(target_os = "linux")]
        {
            Ok(Self::from_io(
                RawHciSocket::open(spec.adapter_index)?,
                spec.adapter_index,
            ))
        }
        #[cfg(not(target_os = "linux"))]
        {
            let _ = spec;
            Err(Error::Unsupported("raw HCI sockets require Linux".into()))
        }
    }
}
