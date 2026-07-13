use crate::{
    Error, FileTransport, PacketSink, PacketSource, Result, SerialTransport, TcpServer,
    TcpTransport, UdpTransport,
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
            if let Some(open) = value.find('[') {
                let close = value[open + 1..]
                    .find(']')
                    .map(|offset| open + 1 + offset)
                    .ok_or_else(|| Error::InvalidSpec("unterminated metadata section".into()))?;
                let contents = &value[open + 1..close];
                let contents = contents.strip_suffix(',').unwrap_or(contents);
                if contents.is_empty() {
                    return Err(Error::InvalidSpec("empty metadata section".into()));
                }
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
    File(FileTransport),
    Serial(SerialTransport),
    Tcp(TcpTransport),
    Udp(UdpTransport),
    #[cfg(unix)]
    Pty(PtyTransport),
    #[cfg(unix)]
    Unix(UnixTransport),
}

impl PacketSource for ExternalTransport {
    fn read_packet(&mut self) -> Result<Option<HciPacket>> {
        match self {
            Self::File(transport) => transport.read_packet(),
            Self::Serial(transport) => transport.read_packet(),
            Self::Tcp(transport) => transport.read_packet(),
            Self::Udp(transport) => transport.read_packet(),
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
            Self::File(transport) => transport.write_packet(packet),
            Self::Serial(transport) => transport.write_packet(packet),
            Self::Tcp(transport) => transport.write_packet(packet),
            Self::Udp(transport) => transport.write_packet(packet),
            #[cfg(unix)]
            Self::Pty(transport) => transport.write_packet(packet),
            #[cfg(unix)]
            Self::Unix(transport) => transport.write_packet(packet),
        }
    }

    fn flush(&mut self) -> Result<()> {
        match self {
            Self::File(transport) => transport.flush(),
            Self::Serial(transport) => transport.flush(),
            Self::Tcp(transport) => transport.flush(),
            Self::Udp(transport) => transport.flush(),
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
        "file" => ExternalTransport::File(FileTransport::open(spec.required_parameters()?)?),
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
    Ok(OpenedTransport {
        transport,
        metadata: spec.metadata,
    })
}
