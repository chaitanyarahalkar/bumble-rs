use crate::{
    Error, FileTransport, PacketSink, PacketSource, Result, SerialTransport,
    SystemAndroidEmulatorTransport, SystemHciSocketTransport, SystemUsbTransport, TcpServer,
    TcpTransport, UdpTransport, VhciTransport, WebSocketServer, WebSocketTransport,
};
use bumble_hci::HciPacket;
use std::collections::BTreeMap;

#[cfg(unix)]
use crate::{PtyTransport, UnixServer, UnixTransport};
#[cfg(unix)]
use std::path::Path;

/// Parsed `<scheme>:[metadata]parameters` transport name.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TransportSpec {
    pub scheme: String,
    pub parameters: Option<String>,
    pub metadata: BTreeMap<String, String>,
}

impl TransportSpec {
    pub fn parse(name: &str) -> Result<Self> {
        let (scheme, parameters) = match name.split_once(':') {
            Some((scheme, parameters)) => (scheme, Some(parameters)),
            None => (name, None),
        };
        if scheme.is_empty() {
            return Err(Error::InvalidSpec("transport scheme is empty".into()));
        }

        let mut parameters = parameters.map(str::to_owned);
        let mut metadata = BTreeMap::new();
        if let Some(value) = parameters.as_mut() {
            let mut search_start = 0;
            let mut metadata_section = None;
            while let Some(offset) = value[search_start..].find('[') {
                let open = search_start + offset;
                let Some(close_offset) = value[open + 1..].find(']') else {
                    if value[open + 1..].contains('=') {
                        return Err(Error::InvalidSpec("unterminated metadata section".into()));
                    }
                    break;
                };
                let close = open + 1 + close_offset;
                let contents = &value[open + 1..close];
                if contents.contains('=') {
                    metadata_section = Some((open, close));
                    break;
                }
                search_start = close + 1;
            }

            if let Some((open, close)) = metadata_section {
                let contents = &value[open + 1..close];
                let contents = contents.strip_suffix(',').unwrap_or(contents);
                for entry in contents.split(',') {
                    let (key, metadata_value) = entry.split_once('=').ok_or_else(|| {
                        Error::InvalidSpec(format!("invalid metadata entry {entry}"))
                    })?;
                    if key.is_empty()
                        || !key
                            .bytes()
                            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
                        || metadata_value.is_empty()
                    {
                        return Err(Error::InvalidSpec(format!(
                            "invalid metadata entry {entry}"
                        )));
                    }
                    metadata.insert(key.into(), metadata_value.into());
                }
                *value = if open == 0 {
                    value[close + 1..].to_owned()
                } else {
                    value[..open].to_owned()
                };
            }
        }

        Ok(Self {
            scheme: scheme.into(),
            parameters,
            metadata,
        })
    }

    fn required_parameters(&self) -> Result<&str> {
        self.parameters
            .as_deref()
            .filter(|parameters| !parameters.is_empty())
            .ok_or_else(|| {
                Error::InvalidSpec(format!("{} transport requires parameters", self.scheme))
            })
    }
}

/// Any packet endpoint supported by [`open_transport`].
pub enum ExternalTransport {
    AndroidEmulator(SystemAndroidEmulatorTransport),
    File(FileTransport),
    HciSocket(SystemHciSocketTransport),
    Serial(SerialTransport),
    Tcp(TcpTransport),
    Udp(UdpTransport),
    Usb(Box<SystemUsbTransport>),
    Vhci(VhciTransport<std::fs::File>),
    WebSocket(Box<WebSocketTransport>),
    #[cfg(unix)]
    Pty(PtyTransport),
    #[cfg(unix)]
    Unix(UnixTransport),
}

impl PacketSource for ExternalTransport {
    fn read_packet(&mut self) -> Result<Option<HciPacket>> {
        match self {
            Self::AndroidEmulator(transport) => transport.read_packet(),
            Self::File(transport) => transport.read_packet(),
            Self::HciSocket(transport) => transport.read_packet(),
            Self::Serial(transport) => transport.read_packet(),
            Self::Tcp(transport) => transport.read_packet(),
            Self::Udp(transport) => transport.read_packet(),
            Self::Usb(transport) => transport.read_packet(),
            Self::Vhci(transport) => transport.read_packet(),
            Self::WebSocket(transport) => transport.read_packet(),
            #[cfg(unix)]
            Self::Pty(transport) => transport.read_packet(),
            #[cfg(unix)]
            Self::Unix(transport) => transport.read_packet(),
        }
    }
}

impl PacketSink for ExternalTransport {
    fn write_packet(&mut self, packet: &HciPacket) -> Result<()> {
        match self {
            Self::AndroidEmulator(transport) => transport.write_packet(packet),
            Self::File(transport) => transport.write_packet(packet),
            Self::HciSocket(transport) => transport.write_packet(packet),
            Self::Serial(transport) => transport.write_packet(packet),
            Self::Tcp(transport) => transport.write_packet(packet),
            Self::Udp(transport) => transport.write_packet(packet),
            Self::Usb(transport) => transport.write_packet(packet),
            Self::Vhci(transport) => transport.write_packet(packet),
            Self::WebSocket(transport) => transport.write_packet(packet),
            #[cfg(unix)]
            Self::Pty(transport) => transport.write_packet(packet),
            #[cfg(unix)]
            Self::Unix(transport) => transport.write_packet(packet),
        }
    }

    fn flush(&mut self) -> Result<()> {
        match self {
            Self::AndroidEmulator(transport) => transport.flush(),
            Self::File(transport) => transport.flush(),
            Self::HciSocket(transport) => transport.flush(),
            Self::Serial(transport) => transport.flush(),
            Self::Tcp(transport) => transport.flush(),
            Self::Udp(transport) => transport.flush(),
            Self::Usb(transport) => transport.flush(),
            Self::Vhci(transport) => transport.flush(),
            Self::WebSocket(transport) => transport.flush(),
            #[cfg(unix)]
            Self::Pty(transport) => transport.flush(),
            #[cfg(unix)]
            Self::Unix(transport) => transport.flush(),
        }
    }
}

pub struct OpenedTransport {
    pub transport: ExternalTransport,
    pub metadata: BTreeMap<String, String>,
}

impl PacketSource for OpenedTransport {
    fn read_packet(&mut self) -> Result<Option<HciPacket>> {
        self.transport.read_packet()
    }
}

impl PacketSink for OpenedTransport {
    fn write_packet(&mut self, packet: &HciPacket) -> Result<()> {
        self.transport.write_packet(packet)
    }

    fn flush(&mut self) -> Result<()> {
        self.transport.flush()
    }
}

/// Open a Bumble transport name.
///
/// In this synchronous port, server schemes block until the first client is
/// accepted. Call [`TcpServer`] or [`UnixServer`] directly when binding and
/// accepting must be separate operations.
pub fn open_transport(name: &str) -> Result<OpenedTransport> {
    let spec = TransportSpec::parse(name)?;
    let transport = match spec.scheme.as_str() {
        "android-emulator" => ExternalTransport::AndroidEmulator(
            SystemAndroidEmulatorTransport::open(spec.parameters.as_deref())?,
        ),
        "file" => ExternalTransport::File(FileTransport::open(spec.required_parameters()?)?),
        "hci-socket" => ExternalTransport::HciSocket(SystemHciSocketTransport::open(
            spec.parameters.as_deref(),
        )?),
        "serial" => ExternalTransport::Serial(SerialTransport::open(spec.required_parameters()?)?),
        "tcp-client" => ExternalTransport::Tcp(TcpTransport::connect(spec.required_parameters()?)?),
        "tcp-server" => {
            let parameters = spec.required_parameters()?;
            let address = parameters
                .strip_prefix("_:")
                .map(|port| format!("0.0.0.0:{port}"))
                .unwrap_or_else(|| parameters.to_owned());
            ExternalTransport::Tcp(TcpServer::bind(address)?.accept()?)
        }
        "udp" => {
            let parameters = spec.required_parameters()?;
            let (local, remote) = parameters
                .split_once(',')
                .ok_or_else(|| Error::InvalidSpec("UDP parameters must be local,remote".into()))?;
            ExternalTransport::Udp(UdpTransport::bind(local, remote)?)
        }
        "usb" | "pyusb" => ExternalTransport::Usb(Box::new(SystemUsbTransport::open(
            spec.required_parameters()?,
        )?)),
        "ws-client" => ExternalTransport::WebSocket(Box::new(WebSocketTransport::connect(
            spec.required_parameters()?,
        )?)),
        "ws-server" => {
            let parameters = spec.required_parameters()?;
            let address = parameters
                .strip_prefix("_:")
                .map(|port| format!("0.0.0.0:{port}"))
                .unwrap_or_else(|| parameters.to_owned());
            ExternalTransport::WebSocket(Box::new(WebSocketServer::bind(address)?.accept()?))
        }
        "vhci" => ExternalTransport::Vhci(VhciTransport::open(
            spec.parameters
                .as_deref()
                .filter(|parameters| !parameters.is_empty())
                .unwrap_or("/dev/vhci"),
        )?),
        #[cfg(unix)]
        "pty" => {
            let link = spec.parameters.as_deref().filter(|value| !value.is_empty());
            ExternalTransport::Pty(PtyTransport::open(link.map(Path::new))?)
        }
        #[cfg(unix)]
        "unix" | "unix-client" => {
            ExternalTransport::Unix(UnixTransport::connect(spec.required_parameters()?)?)
        }
        #[cfg(unix)]
        "unix-server" => {
            ExternalTransport::Unix(UnixServer::bind(spec.required_parameters()?)?.accept()?)
        }
        scheme => return Err(Error::Unsupported(format!("scheme {scheme}"))),
    };
    let mut metadata = spec.metadata;
    if let ExternalTransport::Usb(usb) = &transport {
        metadata.insert("vendor_id".into(), format!("{:04x}", usb.vendor_id()));
        metadata.insert("product_id".into(), format!("{:04x}", usb.product_id()));
        metadata.insert("bus".into(), usb.bus().to_string());
        metadata.insert("address".into(), usb.address().to_string());
    }
    Ok(OpenedTransport {
        transport,
        metadata,
    })
}
